use super::*;
use similar::{Algorithm, DiffOp, DiffTag, capture_diff_slices};
use std::borrow::Cow;
use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;

use skron_git_core::GitObjectStore;

const DIFF_STAT_ROW_INITIAL_CAPACITY_LIMIT: usize = 8192;
const ZERO_SHA1_HEX: &str = "0000000000000000000000000000000000000000";

pub(crate) fn parse_find_renames_option(value: Option<&str>) -> Result<Option<u8>> {
    value
        .map(|value| parse_similarity_threshold("--find-renames", value))
        .transpose()
}

pub(crate) fn parse_find_copies_option(value: Option<&str>) -> Result<Option<u8>> {
    value
        .map(|value| parse_similarity_threshold("--find-copies", value))
        .transpose()
}

pub(crate) fn parse_break_rewrites_option(value: Option<&str>) -> Result<Option<u8>> {
    value
        .map(|value| {
            let rewrite_threshold = value.split_once('/').map(|(_, rewrite)| rewrite);
            match rewrite_threshold {
                Some(rewrite) => parse_similarity_threshold("--break-rewrites", rewrite),
                None => Ok(60),
            }
        })
        .transpose()
}

pub(crate) fn parse_similarity_threshold(option: &str, value: &str) -> Result<u8> {
    if value.is_empty() {
        return Ok(50);
    }
    let percent = value.strip_suffix('%').unwrap_or(value);
    let threshold = if value.ends_with('%') {
        percent.parse::<u8>().ok()
    } else if percent.len() == 1 {
        percent
            .parse::<u8>()
            .ok()
            .map(|value| value.saturating_mul(10))
    } else {
        percent.parse::<u8>().ok()
    };
    match threshold {
        Some(value) if value <= 100 => Ok(value),
        _ => Err(CliError::Fatal {
            code: 129,
            message: format!("invalid {option} threshold '{value}'"),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WordDiffMode {
    None,
    Plain,
    Porcelain,
    Color,
}

pub(crate) fn parse_word_diff_option(value: Option<&str>) -> Result<WordDiffMode> {
    match value {
        None | Some("none") => Ok(WordDiffMode::None),
        Some("") | Some("plain") => Ok(WordDiffMode::Plain),
        Some("porcelain") => Ok(WordDiffMode::Porcelain),
        Some("color") => Ok(WordDiffMode::Color),
        Some(other) => Err(CliError::Stderr {
            code: 129,
            text: format!("error: bad --word-diff argument: {other}\n"),
        }),
    }
}

pub(crate) fn validate_diff_algorithm_options(
    minimal: bool,
    patience: bool,
    histogram: bool,
    diff_algorithm: Option<&str>,
    anchored: &[String],
) -> Result<()> {
    if let Some(algorithm) = diff_algorithm {
        match algorithm {
            "default" | "myers" | "minimal" | "patience" | "histogram" => {}
            _ => {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: option diff-algorithm accepts \"myers\", \"minimal\", \"patience\" and \"histogram\"\n".into(),
                });
            }
        }
    }
    let _accepted_algorithm_options = (minimal, patience, histogram, anchored);
    Ok(())
}

pub(crate) fn diff_entries_for_indexes(
    old_index: &GitIndex,
    new_index: &GitIndex,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
    find_copies_harder: bool,
) -> Result<Vec<skron_git_core::IndexDiffEntry>> {
    if detect_copies == Some(100) {
        Ok(diff_indexes_with_exact_renames_and_copies(
            old_index,
            new_index,
            find_copies_harder,
        )?)
    } else if detect_renames == Some(100) {
        Ok(diff_indexes_with_exact_renames(old_index, new_index)?)
    } else {
        Ok(diff_indexes(old_index, new_index)?)
    }
}

pub(crate) fn diff_entry_matches_pathspec(
    entry: &skron_git_core::IndexDiffEntry,
    pathspecs: &[Vec<u8>],
) -> bool {
    pathspec_matches(&entry.path, pathspecs)
        || entry
            .old_path
            .as_deref()
            .is_some_and(|path| pathspec_matches(path, pathspecs))
}

pub(crate) struct PickaxeOptions<'a> {
    pub(crate) string: Option<&'a str>,
    pub(crate) regex: Option<&'a str>,
    pub(crate) regex_mode: bool,
    pub(crate) all: bool,
}
impl PickaxeOptions<'_> {
    fn enabled(&self) -> bool {
        self.string.is_some() || self.regex.is_some()
    }
}

pub(crate) struct DiffIndexContext<'a> {
    pub(crate) repo: &'a GitRepo,
    pub(crate) store: &'a LooseObjectStore,
    pub(crate) old_index: &'a GitIndex,
    pub(crate) new_index: &'a GitIndex,
    pub(crate) old_source: DiffSideSource,
    pub(crate) new_source: DiffSideSource,
}

pub(crate) struct SimilarityDetectionOptions {
    pub(crate) rename_threshold: Option<u8>,
    pub(crate) copy_threshold: Option<u8>,
    pub(crate) find_copies_harder: bool,
}

pub(crate) fn apply_pickaxe_filter(
    context: &DiffIndexContext<'_>,
    entries: Vec<skron_git_core::IndexDiffEntry>,
    options: PickaxeOptions<'_>,
) -> Result<Vec<skron_git_core::IndexDiffEntry>> {
    if !options.enabled() {
        return Ok(entries);
    }
    let string_regex = options
        .string
        .filter(|_| options.regex_mode)
        .map(Regex::new)
        .transpose()
        .map_err(|error| CliError::Fatal {
            code: 129,
            message: format!("invalid pickaxe regex: {error}"),
        })?;
    let line_regex =
        options
            .regex
            .map(Regex::new)
            .transpose()
            .map_err(|error| CliError::Fatal {
                code: 129,
                message: format!("invalid -G regex: {error}"),
            })?;
    let mut matched = Vec::new();
    for entry in &entries {
        let old_entry = find_index_entry(context.old_index, diff_entry_old_path(entry));
        let new_entry = find_index_entry(context.new_index, &entry.path);
        let old_content = old_entry
            .map(|entry| {
                read_diff_side_content(context.repo, context.store, entry, context.old_source)
            })
            .transpose()?
            .unwrap_or_default();
        let new_content = new_entry
            .map(|entry| {
                read_diff_side_content(context.repo, context.store, entry, context.new_source)
            })
            .transpose()?
            .unwrap_or_default();
        let string_match = if let Some(regex) = string_regex.as_ref() {
            regex.find_iter(&old_content).count() != regex.find_iter(&new_content).count()
        } else if let Some(needle) = options.string {
            count_bytes_occurrences(&old_content, needle.as_bytes())
                != count_bytes_occurrences(&new_content, needle.as_bytes())
        } else {
            false
        };
        let regex_match = line_regex
            .as_ref()
            .is_some_and(|regex| changed_line_matches(&old_content, &new_content, regex));
        if string_match || regex_match {
            matched.push(entry.clone());
        }
    }
    if options.all {
        if matched.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(entries)
        }
    } else {
        Ok(matched)
    }
}

pub(crate) fn apply_similarity_detection(
    context: &DiffIndexContext<'_>,
    entries: Vec<skron_git_core::IndexDiffEntry>,
    options: SimilarityDetectionOptions,
) -> Result<Vec<skron_git_core::IndexDiffEntry>> {
    let SimilarityDetectionOptions {
        rename_threshold,
        copy_threshold,
        find_copies_harder,
    } = options;
    if rename_threshold.is_none_or(|value| value == 100)
        && copy_threshold.is_none_or(|value| value == 100)
    {
        return Ok(entries);
    }
    let mut entries = entries;
    if let Some(threshold) = rename_threshold.filter(|value| *value < 100) {
        entries = detect_similarity_renames(context, entries, threshold)?;
    }
    if let Some(threshold) = copy_threshold.filter(|value| *value < 100) {
        entries = detect_similarity_copies(context, entries, threshold, find_copies_harder)?;
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn detect_similarity_renames(
    context: &DiffIndexContext<'_>,
    entries: Vec<skron_git_core::IndexDiffEntry>,
    threshold: u8,
) -> Result<Vec<skron_git_core::IndexDiffEntry>> {
    let deleted_indexes = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.status == IndexDiffStatus::Deleted)
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();
    let added_indexes = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.status == IndexDiffStatus::Added)
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();
    let mut matches = Vec::new();
    let mut used_old = HashSet::new();
    for new_idx in added_indexes {
        let new_path = &entries[new_idx].path;
        let mut best = None::<(u8, usize)>;
        for old_idx in &deleted_indexes {
            if used_old.contains(old_idx) {
                continue;
            }
            let old_path = &entries[*old_idx].path;
            let score = diff_entry_similarity_score_paths(context, old_path, new_path)?;
            if score >= threshold
                && best
                    .as_ref()
                    .is_none_or(|(best_score, _)| score > *best_score)
            {
                best = Some((score, *old_idx));
            }
        }
        if let Some((score, old_idx)) = best {
            used_old.insert(old_idx);
            matches.push((
                entries[old_idx].path.clone(),
                entries[new_idx].path.clone(),
                score,
            ));
        }
    }
    let used_old_paths = matches
        .iter()
        .map(|(old_path, _, _)| old_path)
        .collect::<HashSet<_>>();
    let used_new_paths = matches
        .iter()
        .map(|(_, new_path, _)| new_path)
        .collect::<HashSet<_>>();
    let mut out = entries
        .into_iter()
        .filter(|entry| {
            !((entry.status == IndexDiffStatus::Deleted && used_old_paths.contains(&entry.path))
                || (entry.status == IndexDiffStatus::Added && used_new_paths.contains(&entry.path)))
        })
        .collect::<Vec<_>>();
    drop(used_old_paths);
    drop(used_new_paths);
    for (old_path, new_path, score) in matches {
        out.push(skron_git_core::IndexDiffEntry {
            status: IndexDiffStatus::Renamed,
            path: new_path,
            old_path: Some(old_path),
            similarity: Some(score),
        });
    }
    Ok(out)
}

fn detect_similarity_copies(
    context: &DiffIndexContext<'_>,
    entries: Vec<skron_git_core::IndexDiffEntry>,
    threshold: u8,
    find_copies_harder: bool,
) -> Result<Vec<skron_git_core::IndexDiffEntry>> {
    let added_indexes = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.status == IndexDiffStatus::Added)
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();
    let mut out = entries;
    if find_copies_harder {
        let source_paths = context
            .old_index
            .entries()
            .iter()
            .filter_map(|entry| (entry.stage == 0).then_some(entry.path.as_slice()))
            .collect::<Vec<_>>();
        for added_idx in added_indexes {
            let best = {
                let added_path = &out[added_idx].path;
                let mut best = None::<(u8, &[u8])>;
                for source_path in &source_paths {
                    let score =
                        diff_entry_similarity_score_paths(context, source_path, added_path)?;
                    if score >= threshold
                        && best
                            .as_ref()
                            .is_none_or(|(best_score, _)| score > *best_score)
                    {
                        best = Some((score, *source_path));
                    }
                }
                best
            };
            if let Some((score, source_path)) = best {
                let entry = &mut out[added_idx];
                entry.status = IndexDiffStatus::Copied;
                entry.old_path = Some(source_path.to_vec());
                entry.similarity = Some(score);
            }
        }
    } else {
        let source_paths = out
            .iter()
            .filter(|entry| {
                matches!(
                    entry.status,
                    IndexDiffStatus::Deleted | IndexDiffStatus::Modified | IndexDiffStatus::Renamed
                )
            })
            .map(|entry| diff_entry_old_path(entry).to_vec())
            .collect::<Vec<_>>();
        for added_idx in added_indexes {
            let best = {
                let added_path = &out[added_idx].path;
                let mut best = None::<(u8, usize)>;
                for (source_idx, source_path) in source_paths.iter().enumerate() {
                    let score =
                        diff_entry_similarity_score_paths(context, source_path, added_path)?;
                    if score >= threshold
                        && best
                            .as_ref()
                            .is_none_or(|(best_score, _)| score > *best_score)
                    {
                        best = Some((score, source_idx));
                    }
                }
                best
            };
            if let Some((score, source_idx)) = best {
                let entry = &mut out[added_idx];
                entry.status = IndexDiffStatus::Copied;
                entry.old_path = Some(source_paths[source_idx].clone());
                entry.similarity = Some(score);
            }
        }
    }
    Ok(out)
}

fn diff_entry_similarity_score(
    context: &DiffIndexContext<'_>,
    old_entry: &skron_git_core::IndexDiffEntry,
    new_entry: &skron_git_core::IndexDiffEntry,
) -> Result<u8> {
    diff_entry_similarity_score_paths(context, diff_entry_old_path(old_entry), &new_entry.path)
}

fn diff_entry_similarity_score_paths(
    context: &DiffIndexContext<'_>,
    old_path: &[u8],
    new_path: &[u8],
) -> Result<u8> {
    let Some(old_index_entry) = find_index_entry(context.old_index, old_path) else {
        return Ok(0);
    };
    let Some(new_index_entry) = find_index_entry(context.new_index, new_path) else {
        return Ok(0);
    };
    if old_index_entry.mode != new_index_entry.mode {
        return Ok(0);
    }
    let old_content = read_diff_side_content(
        context.repo,
        context.store,
        old_index_entry,
        context.old_source,
    )?;
    let new_content = read_diff_side_content(
        context.repo,
        context.store,
        new_index_entry,
        context.new_source,
    )?;
    Ok(content_similarity_score(&old_content, &new_content))
}

pub(crate) fn apply_break_rewrites(
    context: &DiffIndexContext<'_>,
    entries: Vec<skron_git_core::IndexDiffEntry>,
    threshold: Option<u8>,
) -> Result<Vec<skron_git_core::IndexDiffEntry>> {
    let Some(threshold) = threshold else {
        return Ok(entries);
    };
    entries
        .into_iter()
        .map(|mut entry| {
            if entry.status == IndexDiffStatus::Modified {
                let score = diff_entry_similarity_score(context, &entry, &entry)?;
                let dissimilarity = 100_u8.saturating_sub(score);
                if dissimilarity >= threshold {
                    entry.similarity = Some(dissimilarity);
                }
            }
            Ok(entry)
        })
        .collect()
}

pub(crate) fn content_similarity_score(old_content: &[u8], new_content: &[u8]) -> u8 {
    if old_content == new_content {
        return 100;
    }
    if old_content.is_empty() && new_content.is_empty() {
        return 100;
    }
    if is_binary_content(old_content) || is_binary_content(new_content) {
        return 0;
    }
    let old_lines = split_diff_lines(old_content);
    let new_lines = split_diff_lines(new_content);
    if old_lines.is_empty() && new_lines.is_empty() {
        return 100;
    }
    let common = lcs_line_bytes(&old_lines, &new_lines);
    ((common * 100) / old_content.len().max(new_content.len())) as u8
}

pub(crate) fn lcs_line_bytes(left: &[&[u8]], right: &[&[u8]]) -> usize {
    let mut row = vec![0usize; right.len() + 1];
    for left_line in left {
        let mut previous_diagonal = 0usize;
        for (idx, right_line) in right.iter().enumerate() {
            let previous_above = row[idx + 1];
            row[idx + 1] = if left_line == right_line {
                previous_diagonal + left_line.len()
            } else {
                row[idx + 1].max(row[idx])
            };
            previous_diagonal = previous_above;
        }
    }
    row[right.len()]
}

pub(crate) fn count_bytes_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

pub(crate) fn changed_line_matches(old_content: &[u8], new_content: &[u8], regex: &Regex) -> bool {
    let old_lines = split_diff_lines(old_content);
    let new_lines = split_diff_lines(new_content);
    diff_line_ops(&old_lines, &new_lines)
        .into_iter()
        .any(|op| match op {
            DiffLineOp::Delete(line) | DiffLineOp::Insert(line) => regex.is_match(line),
            DiffLineOp::Equal(_) => false,
        })
}

pub(crate) fn apply_diff_order_file(
    entries: Vec<skron_git_core::IndexDiffEntry>,
    order_file: Option<&Path>,
) -> Result<Vec<skron_git_core::IndexDiffEntry>> {
    let Some(order_file) = order_file else {
        return Ok(entries);
    };
    let patterns = read_diff_order_patterns(order_file)?;
    if patterns.is_empty() {
        return Ok(entries);
    }
    let mut ranked = entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let rank = diff_order_rank(&entry, &patterns).unwrap_or(usize::MAX);
            (rank, index, entry)
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(rank, index, _)| (*rank, *index));
    Ok(ranked.into_iter().map(|(_, _, entry)| entry).collect())
}

pub(crate) fn read_diff_order_patterns(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)?;
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect())
}

pub(crate) fn diff_order_rank(
    entry: &skron_git_core::IndexDiffEntry,
    patterns: &[String],
) -> Option<usize> {
    patterns.iter().position(|pattern| {
        diff_order_pattern_matches(pattern, &entry.path)
            || entry
                .old_path
                .as_deref()
                .is_some_and(|old_path| diff_order_pattern_matches(pattern, old_path))
    })
}

pub(crate) fn diff_order_pattern_matches(pattern: &str, path: &[u8]) -> bool {
    let path = String::from_utf8_lossy(path).replace('\\', "/");
    let pattern = pattern.replace('\\', "/");
    if pattern == path {
        return true;
    }
    wildcard_match_pathspec(&pattern, &path, false, true)
        || path
            .rsplit('/')
            .next()
            .is_some_and(|basename| wildcard_match_pathspec(&pattern, basename, false, true))
}

pub(crate) fn apply_diff_skip_rotate(
    mut entries: Vec<skron_git_core::IndexDiffEntry>,
    skip_to: Option<&str>,
    rotate_to: Option<&str>,
) -> Vec<skron_git_core::IndexDiffEntry> {
    if let Some(target) = rotate_to
        && let Some(position) = entries
            .iter()
            .position(|entry| diff_entry_matches_name(entry, target))
    {
        entries.rotate_left(position);
    }
    if let Some(target) = skip_to
        && let Some(position) = entries
            .iter()
            .position(|entry| diff_entry_matches_name(entry, target))
    {
        entries.drain(..position);
    }
    entries
}

pub(crate) fn diff_entry_matches_name(
    entry: &skron_git_core::IndexDiffEntry,
    target: &str,
) -> bool {
    diff_display_path(&entry.path, None) == target
        || entry
            .old_path
            .as_deref()
            .is_some_and(|path| diff_display_path(path, None) == target)
}

pub(crate) struct NoIndexDiffEntry {
    pub(crate) status: IndexDiffStatus,
    pub(crate) old_display: String,
    pub(crate) new_display: String,
    pub(crate) old_label: String,
    pub(crate) new_label: String,
    pub(crate) stat_path: String,
    pub(crate) name_only_path: String,
    pub(crate) name_status_path: String,
    pub(crate) old_content: Vec<u8>,
    pub(crate) new_content: Vec<u8>,
    pub(crate) old_is_null: bool,
    pub(crate) new_is_null: bool,
}

#[derive(Clone)]
pub(crate) struct RootTreeDiffEntry {
    pub(crate) status: IndexDiffStatus,
    pub(crate) path: Vec<u8>,
    pub(crate) old_mode: Option<TreeMode>,
    pub(crate) new_mode: Option<TreeMode>,
    pub(crate) old_id: Option<ObjectId>,
    pub(crate) new_id: Option<ObjectId>,
}

pub(crate) struct RootTreeRenderOptions<'a> {
    pub(crate) diff_filter: DiffFilter,
    pub(crate) order_file: Option<&'a Path>,
    pub(crate) skip_to: Option<&'a str>,
    pub(crate) rotate_to: Option<&'a str>,
    pub(crate) relative_prefix: Option<&'a [u8]>,
    pub(crate) options: &'a DiffRenderOptions,
}

pub(crate) fn diff_no_index(options: &DiffOptions) -> Result<()> {
    let paths = &options.paths;
    if paths.len() != 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "diff --no-index requires exactly two paths".into(),
        });
    }
    let left = absolute_path_from_arg(&paths[0])?;
    let right = absolute_path_from_arg(&paths[1])?;
    for (original, path) in [(&paths[0], &left), (&paths[1], &right)] {
        if !path_exists(path) {
            return Err(CliError::Stderr {
                code: 1,
                text: format!("error: Could not access '{}'\n", original.display()),
            });
        }
    }
    if left.is_dir() != right.is_dir() {
        let missing = if left.is_dir() {
            paths[0].join(paths[1].file_name().unwrap_or(paths[1].as_os_str()))
        } else {
            paths[1].join(paths[0].file_name().unwrap_or(paths[0].as_os_str()))
        };
        return Err(CliError::Stderr {
            code: 1,
            text: format!("error: Could not access '{}'\n", missing.display()),
        });
    }
    let entries = collect_no_index_entries(&paths[0], &left, &paths[1], &right)?;
    if entries.is_empty() {
        return Ok(());
    }
    render_no_index_entries(options, entries)
}

pub(crate) fn collect_no_index_entries(
    left_arg: &Path,
    left: &Path,
    right_arg: &Path,
    right: &Path,
) -> Result<Vec<NoIndexDiffEntry>> {
    if left.is_dir() || right.is_dir() {
        return collect_no_index_directory_entries(left_arg, left, right_arg, right);
    }
    let left_content = fs::read(left)?;
    let right_content = fs::read(right)?;
    if left_content == right_content {
        return Ok(Vec::new());
    }
    let left_is_null = is_null_diff_path(left_arg);
    let right_is_null = is_null_diff_path(right_arg);
    let left_display = if left_is_null {
        right_arg.display().to_string()
    } else {
        left_arg.display().to_string()
    };
    let right_display = if right_is_null {
        left_arg.display().to_string()
    } else {
        right_arg.display().to_string()
    };
    let status = if left_is_null {
        IndexDiffStatus::Added
    } else if right_is_null {
        IndexDiffStatus::Deleted
    } else {
        IndexDiffStatus::Modified
    };
    Ok(vec![NoIndexDiffEntry {
        status,
        old_display: left_display.clone(),
        new_display: right_display.clone(),
        old_label: if left_is_null {
            "/dev/null".to_owned()
        } else {
            format!("a/{left_display}")
        },
        new_label: if right_is_null {
            "/dev/null".to_owned()
        } else {
            format!("b/{right_display}")
        },
        stat_path: no_index_stat_path(status, &left_display, &right_display),
        name_only_path: if matches!(status, IndexDiffStatus::Deleted) {
            "/dev/null".to_owned()
        } else {
            right_display.clone()
        },
        name_status_path: if matches!(status, IndexDiffStatus::Added) {
            right_display.clone()
        } else {
            left_display.clone()
        },
        old_content: left_content,
        new_content: right_content,
        old_is_null: left_is_null,
        new_is_null: right_is_null,
    }])
}

pub(crate) fn collect_no_index_directory_entries(
    left_arg: &Path,
    left: &Path,
    right_arg: &Path,
    right: &Path,
) -> Result<Vec<NoIndexDiffEntry>> {
    let mut rels = BTreeSet::new();
    if left.is_dir() {
        collect_no_index_relative_files(left, left, &mut rels)?;
    }
    if right.is_dir() {
        collect_no_index_relative_files(right, right, &mut rels)?;
    }
    let mut entries = Vec::new();
    for rel in rels {
        let left_path = left.join(&rel);
        let right_path = right.join(&rel);
        let left_exists = left_path.is_file();
        let right_exists = right_path.is_file();
        let old_content = if left_exists {
            fs::read(&left_path)?
        } else {
            Vec::new()
        };
        let new_content = if right_exists {
            fs::read(&right_path)?
        } else {
            Vec::new()
        };
        if left_exists && right_exists && old_content == new_content {
            continue;
        }
        let rel_display = rel.display().to_string();
        let left_display = left_arg.join(&rel).display().to_string();
        let right_display = right_arg.join(&rel).display().to_string();
        let status = if !left_exists {
            IndexDiffStatus::Added
        } else if !right_exists {
            IndexDiffStatus::Deleted
        } else {
            IndexDiffStatus::Modified
        };
        entries.push(NoIndexDiffEntry {
            status,
            old_display: if left_exists {
                left_display.clone()
            } else {
                right_display.clone()
            },
            new_display: if right_exists {
                right_display.clone()
            } else {
                left_display.clone()
            },
            old_label: if left_exists {
                format!("a/{left_display}")
            } else {
                "/dev/null".to_owned()
            },
            new_label: if right_exists {
                format!("b/{right_display}")
            } else {
                "/dev/null".to_owned()
            },
            stat_path: match status {
                IndexDiffStatus::Added => format!("/dev/null => {right_display}"),
                IndexDiffStatus::Deleted => format!("{left_display} => /dev/null"),
                _ => no_index_directory_stat_path(left_arg, right_arg, &rel_display),
            },
            name_only_path: match status {
                IndexDiffStatus::Deleted => "/dev/null".to_owned(),
                _ => right_display.clone(),
            },
            name_status_path: match status {
                IndexDiffStatus::Added => right_display,
                _ => left_display,
            },
            old_content,
            new_content,
            old_is_null: !left_exists,
            new_is_null: !right_exists,
        });
    }
    Ok(entries)
}

pub(crate) fn collect_no_index_relative_files(
    root: &Path,
    dir: &Path,
    rels: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_no_index_relative_files(root, &path, rels)?;
        } else if path.is_file() {
            let rel = path.strip_prefix(root).map_err(|_| CliError::Fatal {
                code: 128,
                message: "failed to compute no-index relative path".into(),
            })?;
            rels.insert(rel.to_path_buf());
        }
    }
    Ok(())
}

pub(crate) fn render_no_index_entries(
    options: &DiffOptions,
    mut entries: Vec<NoIndexDiffEntry>,
) -> Result<()> {
    if options.quiet {
        return Err(CliError::Exit(1));
    }
    if options.reverse {
        for entry in &mut entries {
            reverse_no_index_entry(entry);
        }
    }
    let binary = |entry: &NoIndexDiffEntry| {
        !options.text
            && (is_binary_content(&entry.old_content) || is_binary_content(&entry.new_content))
    };
    let ignore_matching_lines = compile_ignore_matching_lines(&options.ignore_matching_lines)?;
    let whitespace_mode = diff_whitespace_mode(
        options.ignore_space_at_eol,
        options.ignore_cr_at_eol,
        options.ignore_space_change,
        options.ignore_all_space,
        options.ignore_blank_lines,
    );
    let rows = entries
        .iter()
        .map(|entry| {
            let is_binary = binary(entry);
            let (insertions, deletions) = if is_binary {
                (0, 0)
            } else {
                diff_line_counts_with_options(
                    &entry.old_content,
                    &entry.new_content,
                    whitespace_mode,
                    &ignore_matching_lines,
                )
            };
            (entry, is_binary, insertions, deletions)
        })
        .filter(|(_, is_binary, insertions, deletions)| {
            *is_binary || *insertions != 0 || *deletions != 0
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Ok(());
    }
    if options.name_only {
        for (entry, _, _, _) in &rows {
            println!("{}", entry.name_only_path);
        }
        return Err(CliError::Exit(1));
    }
    if options.name_status {
        for (entry, _, _, _) in &rows {
            println!("{}\t{}", entry.status.name_status(), entry.name_status_path);
        }
        return Err(CliError::Exit(1));
    }
    if options.numstat {
        for (entry, is_binary, insertions, deletions) in &rows {
            if *is_binary {
                println!("-\t-\t{}", entry.stat_path);
            } else {
                println!("{insertions}\t{deletions}\t{}", entry.stat_path);
            }
        }
        return Err(CliError::Exit(1));
    }
    if options.summary {
        print_no_index_summary(&rows);
        return Err(CliError::Exit(1));
    }
    if options.raw {
        print_no_index_raw(&rows);
        return Err(CliError::Exit(1));
    }
    let stat_rows = rows
        .iter()
        .map(|(entry, is_binary, insertions, deletions)| DiffStatRow {
            path: entry.stat_path.clone(),
            old_bytes: entry.old_content.len(),
            new_bytes: entry.new_content.len(),
            insertions: *insertions,
            deletions: *deletions,
            binary: *is_binary,
        })
        .collect::<Vec<_>>();
    if options.shortstat {
        print_diff_stat_summary(&stat_rows);
        return Err(CliError::Exit(1));
    }
    if options.stat {
        print_no_index_stat_rows(&stat_rows);
        return Err(CliError::Exit(1));
    }
    if options.no_patch {
        return Err(CliError::Exit(1));
    }
    if options.patch_with_stat {
        print_no_index_stat_rows(&stat_rows);
        println!();
    } else if options.patch_with_raw {
        print_no_index_raw(&rows);
        println!();
    }
    write_no_index_patches(options, &rows, &ignore_matching_lines, whitespace_mode)?;
    Err(CliError::Exit(1))
}

pub(crate) fn reverse_no_index_entry(entry: &mut NoIndexDiffEntry) {
    std::mem::swap(&mut entry.old_display, &mut entry.new_display);
    std::mem::swap(&mut entry.old_label, &mut entry.new_label);
    std::mem::swap(&mut entry.old_content, &mut entry.new_content);
    std::mem::swap(&mut entry.old_is_null, &mut entry.new_is_null);
    entry.status = match entry.status {
        IndexDiffStatus::Added => IndexDiffStatus::Deleted,
        IndexDiffStatus::Deleted => IndexDiffStatus::Added,
        status => status,
    };
    entry.stat_path = no_index_stat_path(entry.status, &entry.old_display, &entry.new_display);
    entry.name_only_path = if matches!(entry.status, IndexDiffStatus::Deleted) {
        "/dev/null".to_owned()
    } else {
        entry.new_display.clone()
    };
    entry.name_status_path = if matches!(entry.status, IndexDiffStatus::Added) {
        entry.new_display.clone()
    } else {
        entry.old_display.clone()
    };
}

pub(crate) fn print_no_index_raw(rows: &[(&NoIndexDiffEntry, bool, usize, usize)]) {
    for (entry, _, _, _) in rows {
        let old_mode = if entry.old_is_null {
            "000000"
        } else {
            "100644"
        };
        let new_mode = if entry.new_is_null {
            "000000"
        } else {
            "100644"
        };
        let old_hash = match entry.status {
            IndexDiffStatus::Deleted => blob_hash_for_diff(&entry.old_content, false, false),
            IndexDiffStatus::Added => "0000000".to_owned(),
            _ => "0000000".to_owned(),
        };
        let new_hash = match entry.status {
            IndexDiffStatus::Added => blob_hash_for_diff(&entry.new_content, false, false),
            IndexDiffStatus::Deleted => "0000000".to_owned(),
            _ => "0000000".to_owned(),
        };
        println!(
            ":{old_mode} {new_mode} {old_hash} {new_hash} {}\t{}",
            entry.status.name_status(),
            entry.name_status_path
        );
    }
}

pub(crate) fn print_no_index_summary(rows: &[(&NoIndexDiffEntry, bool, usize, usize)]) {
    for (entry, _, _, _) in rows {
        match entry.status {
            IndexDiffStatus::Added => println!(" create mode 100644 {}", entry.new_display),
            IndexDiffStatus::Deleted => println!(" delete mode 100644 {}", entry.old_display),
            _ => {}
        }
    }
}

pub(crate) fn write_no_index_patches(
    options: &DiffOptions,
    rows: &[(&NoIndexDiffEntry, bool, usize, usize)],
    ignore_matching_lines: &[Regex],
    whitespace_mode: DiffWhitespaceMode,
) -> Result<()> {
    let mut out = io::stdout().lock();
    for (entry, is_binary, _, _) in rows {
        let (old_prefix, new_prefix) = if options.reverse {
            ("b/", "a/")
        } else {
            ("a/", "b/")
        };
        writeln!(
            out,
            "diff --git {old_prefix}{} {new_prefix}{}",
            entry.old_display, entry.new_display
        )?;
        let full_index = options.binary && *is_binary;
        let left_hash = blob_hash_for_diff(&entry.old_content, entry.old_is_null, full_index);
        let right_hash = blob_hash_for_diff(&entry.new_content, entry.new_is_null, full_index);
        if entry.old_is_null {
            writeln!(out, "new file mode 100644")?;
        } else if entry.new_is_null {
            writeln!(out, "deleted file mode 100644")?;
        }
        if entry.old_is_null || entry.new_is_null {
            writeln!(out, "index {left_hash}..{right_hash}")?;
        } else {
            writeln!(out, "index {left_hash}..{right_hash} 100644")?;
        }
        if *is_binary {
            if options.binary {
                write_git_binary_patch(&mut out, &entry.new_content, &entry.old_content)?;
            } else {
                writeln!(
                    out,
                    "Binary files {} and {} differ",
                    entry.old_label, entry.new_label
                )?;
            }
        } else if options.irreversible_delete && entry.status == IndexDiffStatus::Deleted {
            continue;
        } else {
            writeln!(out, "--- {}", entry.old_label)?;
            writeln!(out, "+++ {}", entry.new_label)?;
            let unified_context = options
                .unified
                .as_deref()
                .map(|value| parse_diff_context_value("--unified", value))
                .transpose()?
                .unwrap_or(3);
            let inter_hunk_context = options
                .inter_hunk_context
                .as_deref()
                .map(|value| parse_diff_context_value("--inter-hunk-context", value))
                .transpose()?
                .unwrap_or(0);
            write_unified_full_file_hunk(
                &mut out,
                &entry.old_content,
                &entry.new_content,
                "",
                HunkFormatOptions {
                    word_diff: parse_word_diff_option(options.word_diff.as_deref())?,
                    color: false,
                    unified_context,
                    inter_hunk_context,
                    output_indicator_new: parse_output_indicator(
                        "--output-indicator-new",
                        options.output_indicator_new.as_deref(),
                    )?,
                    output_indicator_old: parse_output_indicator(
                        "--output-indicator-old",
                        options.output_indicator_old.as_deref(),
                    )?,
                    output_indicator_context: parse_output_indicator(
                        "--output-indicator-context",
                        options.output_indicator_context.as_deref(),
                    )?,
                    ignore_matching_lines,
                    whitespace_mode,
                    emit_hunk_headers: true,
                },
            )?;
        }
    }
    Ok(())
}

pub(crate) fn no_index_stat_path(
    status: IndexDiffStatus,
    old_display: &str,
    new_display: &str,
) -> String {
    match status {
        IndexDiffStatus::Added => format!("/dev/null => {new_display}"),
        IndexDiffStatus::Deleted => format!("{old_display} => /dev/null"),
        _ => no_index_rewrite_stat_path(old_display, new_display),
    }
}

pub(crate) fn no_index_directory_stat_path(
    left_arg: &Path,
    right_arg: &Path,
    rel_display: &str,
) -> String {
    let left = left_arg.display().to_string();
    let right = right_arg.display().to_string();
    format!("{{{left} => {right}}}/{rel_display}")
}

pub(crate) fn no_index_rewrite_stat_path(old_display: &str, new_display: &str) -> String {
    let old_parts = old_display.split('/').collect::<Vec<_>>();
    let new_parts = new_display.split('/').collect::<Vec<_>>();
    let mut suffix_len = 0usize;
    while suffix_len < old_parts.len().min(new_parts.len())
        && old_parts[old_parts.len() - 1 - suffix_len]
            == new_parts[new_parts.len() - 1 - suffix_len]
    {
        suffix_len += 1;
    }
    if suffix_len > 0 && suffix_len < old_parts.len() && suffix_len < new_parts.len() {
        let old_prefix = old_parts[..old_parts.len() - suffix_len].join("/");
        let new_prefix = new_parts[..new_parts.len() - suffix_len].join("/");
        let suffix = old_parts[old_parts.len() - suffix_len..].join("/");
        return format!("{{{old_prefix} => {new_prefix}}}/{suffix}");
    }
    format!("{old_display} => {new_display}")
}

pub(crate) fn is_null_diff_path(path: &std::path::Path) -> bool {
    path == std::path::Path::new("/dev/null")
}

pub(crate) fn blob_hash_for_diff(content: &[u8], is_null: bool, full: bool) -> String {
    if is_null {
        if full {
            zero_object_id().to_hex()
        } else {
            "0000000".to_owned()
        }
    } else {
        let hash = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, content).to_hex();
        if full { hash } else { hash[..7].to_owned() }
    }
}

pub(crate) fn print_no_index_stat_rows(rows: &[DiffStatRow]) {
    let path_width = rows.iter().map(|row| row.path.len()).max().unwrap_or(0);
    let change_width = rows
        .iter()
        .filter(|row| !row.binary)
        .map(|row| row.insertions + row.deletions)
        .max()
        .unwrap_or(0)
        .to_string()
        .len()
        .max(if rows.iter().any(|row| row.binary) {
            3
        } else {
            0
        });
    let max_changes = rows
        .iter()
        .filter(|row| !row.binary)
        .map(|row| row.insertions + row.deletions)
        .max()
        .unwrap_or(0);
    for row in rows {
        let path_padding = " ".repeat(path_width.saturating_sub(row.path.len()));
        if row.binary {
            println!(
                " {}{} | Bin {} -> {} bytes",
                row.path, path_padding, row.old_bytes, row.new_bytes
            );
        } else {
            let changes = row.insertions + row.deletions;
            let graph = stat_graph(row.insertions, row.deletions, max_changes, changes.max(1));
            if graph.is_empty() {
                println!(" {}{} | {:>change_width$}", row.path, path_padding, changes);
            } else {
                println!(
                    " {}{} | {:>change_width$} {}",
                    row.path, path_padding, changes, graph
                );
            }
        }
    }
    print_diff_stat_summary(rows);
}

pub(crate) fn diff_check(
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[skron_git_core::IndexDiffEntry],
    old_source: DiffSideSource,
    new_source: DiffSideSource,
) -> Result<()> {
    let mut errors = 0usize;
    for entry in entries {
        let old_content = find_index_entry(old_index, diff_entry_old_path(entry))
            .map(|entry| read_diff_side_content(repo, store, entry, old_source))
            .transpose()?
            .unwrap_or_default();
        let Some(new_entry) = find_index_entry(new_index, &entry.path) else {
            continue;
        };
        let new_content = read_diff_side_content(repo, store, new_entry, new_source)?;
        if is_binary_content(&old_content) || is_binary_content(&new_content) {
            continue;
        }
        errors += print_diff_check_errors(&entry.path, &old_content, &new_content)?;
    }
    if errors > 0 {
        Err(CliError::Exit(2))
    } else {
        Ok(())
    }
}

pub(crate) fn print_diff_check_errors(
    path: &[u8],
    old_content: &[u8],
    new_content: &[u8],
) -> Result<usize> {
    let old_lines = split_diff_lines(old_content);
    let new_lines = split_diff_lines(new_content);
    let mut line_number = 0usize;
    let mut errors = 0usize;
    for op in diff_line_ops(&old_lines, &new_lines) {
        match op {
            DiffLineOp::Equal(_) | DiffLineOp::Insert(_) => {
                line_number += 1;
            }
            DiffLineOp::Delete(_) => {}
        }
        let DiffLineOp::Insert(line) = op else {
            continue;
        };
        if !line_has_trailing_whitespace(line) {
            continue;
        }
        errors += 1;
        println!(
            "{}:{line_number}: trailing whitespace.",
            String::from_utf8_lossy(path)
        );
        print!("+{}", String::from_utf8_lossy(line));
        if !line.ends_with(b"\n") {
            println!();
        }
    }
    Ok(errors)
}

pub(crate) fn line_has_trailing_whitespace(line: &[u8]) -> bool {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    line.ends_with(b" ") || line.ends_with(b"\t")
}

pub(crate) struct DiffRenderOptions {
    pub(crate) stat: bool,
    pub(crate) patch_with_raw: bool,
    pub(crate) patch_with_stat: bool,
    pub(crate) numstat: bool,
    pub(crate) shortstat: bool,
    pub(crate) raw: bool,
    pub(crate) summary: bool,
    pub(crate) name_status: bool,
    pub(crate) name_only: bool,
    pub(crate) nul_terminated: bool,
    pub(crate) patch: bool,
    pub(crate) no_patch: bool,
    pub(crate) binary: bool,
    pub(crate) quiet: bool,
    pub(crate) exit_code: bool,
    pub(crate) raw_abbrev_len: Option<usize>,
    pub(crate) word_diff: WordDiffMode,
    pub(crate) patch_abbrev_len: Option<usize>,
    pub(crate) old_prefix: String,
    pub(crate) new_prefix: String,
    pub(crate) unified_context: usize,
    pub(crate) inter_hunk_context: usize,
    pub(crate) output_indicator_new: Option<u8>,
    pub(crate) output_indicator_old: Option<u8>,
    pub(crate) output_indicator_context: Option<u8>,
    pub(crate) ignore_matching_lines: Vec<Regex>,
    pub(crate) whitespace_mode: DiffWhitespaceMode,
    pub(crate) relative_prefix: Option<Vec<u8>>,
    pub(crate) text: bool,
    pub(crate) irreversible_delete: bool,
    pub(crate) submodule_format: SubmoduleDiffFormat,
    pub(crate) color_mode: DiffColorMode,
    pub(crate) old_source: DiffSideSource,
    pub(crate) new_source: DiffSideSource,
}

impl DiffRenderOptions {
    pub(crate) fn validate_format(&self, include_patch: bool) -> Result<()> {
        if [
            self.stat,
            self.patch_with_raw,
            self.patch_with_stat,
            self.numstat,
            self.shortstat,
            self.raw,
            self.summary,
            self.name_status,
            self.name_only,
            include_patch && self.patch,
        ]
        .into_iter()
        .filter(|selected| *selected)
        .count()
            > 1
        {
            return Err(CliError::Fatal {
                code: 129,
                message:
                    "diff output format must be one of --stat, --numstat, --shortstat, --raw, --summary, --name-status, --name-only or --patch"
                        .into(),
            });
        }
        Ok(())
    }

    pub(crate) fn reverse_direction(&mut self) {
        std::mem::swap(&mut self.old_prefix, &mut self.new_prefix);
        std::mem::swap(&mut self.old_source, &mut self.new_source);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffSideSource {
    Index,
    WorktreeOrIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubmoduleDiffFormat {
    Short,
    Log,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IgnoreSubmodulesMode {
    None,
    Untracked,
    Dirty,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffColorMode {
    Never,
    Always,
    Auto,
}

impl DiffColorMode {
    fn enabled(self) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::Auto => io::stdout().is_terminal(),
        }
    }
}

pub(crate) fn parse_submodule_diff_format(value: Option<&str>) -> Result<SubmoduleDiffFormat> {
    match value.unwrap_or("short") {
        "short" => Ok(SubmoduleDiffFormat::Short),
        "log" => Ok(SubmoduleDiffFormat::Log),
        "diff" => Ok(SubmoduleDiffFormat::Diff),
        other => Err(CliError::Stderr {
            code: 129,
            text: format!("error: failed to parse --submodule option parameter: '{other}'\n"),
        }),
    }
}

pub(crate) fn parse_ignore_submodules_mode(value: Option<&str>) -> Result<IgnoreSubmodulesMode> {
    match value {
        None | Some("none") => Ok(IgnoreSubmodulesMode::None),
        Some("") | Some("all") => Ok(IgnoreSubmodulesMode::All),
        Some("untracked") => Ok(IgnoreSubmodulesMode::Untracked),
        Some("dirty") => Ok(IgnoreSubmodulesMode::Dirty),
        Some(other) => Err(CliError::Fatal {
            code: 128,
            message: format!("bad --ignore-submodules argument: {other}"),
        }),
    }
}

pub(crate) fn diff_entry_is_gitlink(
    entry: &skron_git_core::IndexDiffEntry,
    old_index: &GitIndex,
    new_index: &GitIndex,
) -> bool {
    find_index_entry(old_index, diff_entry_old_path(entry))
        .is_some_and(|entry| entry.mode == IndexMode::Gitlink)
        || find_index_entry(new_index, &entry.path)
            .is_some_and(|entry| entry.mode == IndexMode::Gitlink)
}

pub(crate) fn filter_ignored_submodule_entries(
    entries: Vec<skron_git_core::IndexDiffEntry>,
    old_index: &GitIndex,
    new_index: &GitIndex,
    mode: IgnoreSubmodulesMode,
) -> Vec<skron_git_core::IndexDiffEntry> {
    if mode != IgnoreSubmodulesMode::All {
        return entries;
    }
    entries
        .into_iter()
        .filter(|entry| !diff_entry_is_gitlink(entry, old_index, new_index))
        .collect()
}

impl DiffSideSource {
    fn raw_id(self, entry: Option<&IndexEntry>, abbrev_len: usize) -> String {
        match self {
            DiffSideSource::Index => entry
                .map(|entry| short_object_id_len(&entry.id, abbrev_len))
                .unwrap_or_else(|| short_zero_object_id_len(abbrev_len)),
            DiffSideSource::WorktreeOrIndex => short_zero_object_id_len(abbrev_len),
        }
    }
}

pub(crate) fn diff_side_sources(new_side_from_index: bool) -> (DiffSideSource, DiffSideSource) {
    (
        DiffSideSource::Index,
        if new_side_from_index {
            DiffSideSource::Index
        } else {
            DiffSideSource::WorktreeOrIndex
        },
    )
}

pub(crate) fn read_diff_side_content(
    repo: &GitRepo,
    store: &LooseObjectStore,
    entry: &IndexEntry,
    source: DiffSideSource,
) -> Result<Vec<u8>> {
    match source {
        DiffSideSource::Index => {
            let _trace = phase_trace("format_patch.write_tree_diff.entry_content.index");
            read_index_entry_content(store, entry)
        }
        DiffSideSource::WorktreeOrIndex => {
            let _trace =
                phase_trace("format_patch.write_tree_diff.entry_content.worktree_or_index");
            read_worktree_or_index_entry_content(repo, store, entry)
        }
    }
}

pub(crate) struct FormatPatchBlobCache<'a> {
    store: &'a LooseObjectStore,
    cached_blobs: HashMap<ObjectId, Box<[u8]>>,
    cached_blob_sizes: HashMap<ObjectId, Option<usize>>,
}

impl<'a> FormatPatchBlobCache<'a> {
    pub(crate) fn new(store: &'a LooseObjectStore) -> Self {
        Self {
            store,
            cached_blobs: HashMap::new(),
            cached_blob_sizes: HashMap::new(),
        }
    }

    pub(crate) fn ensure_index_blob(&mut self, entry: &IndexEntry) -> Result<()> {
        if entry.mode == IndexMode::Gitlink {
            return Ok(());
        }
        let Entry::Vacant(cached) = self.cached_blobs.entry(entry.id.clone()) else {
            return Ok(());
        };
        let object = {
            let _trace = phase_trace("format_patch.write_tree_diff.entry_ensure_blobs.read_object");
            self.store.packed_first().read_object(&entry.id)?
        };
        if object.kind != GitObjectKind::Blob {
            return Err(CliError::Fatal {
                code: 128,
                message: "diff index entry does not point to a blob".into(),
            });
        }
        let content = object.content.into_boxed_slice();
        cached.insert(content);
        Ok(())
    }

    pub(crate) fn index_blob_or_empty<'b>(&'b self, entry: &IndexEntry) -> &'b [u8] {
        if entry.mode == IndexMode::Gitlink {
            return &[];
        }
        self.cached_blobs
            .get(&entry.id)
            .expect("index blob ensured before access")
            .as_ref()
    }

    pub(crate) fn index_blob_size_hint(&mut self, entry: &IndexEntry) -> Result<Option<usize>> {
        if entry.mode == IndexMode::Gitlink {
            return Ok(Some(0));
        }
        if let Some(content) = self.cached_blobs.get(&entry.id) {
            return Ok(Some(content.len()));
        }
        if entry.size != 0 {
            return Ok(Some(entry.size as usize));
        }
        if let Some(size) = self.cached_blob_sizes.get(&entry.id) {
            return Ok(*size);
        }
        let size = {
            let _trace =
                phase_trace("format_patch.write_tree_diff.entry_binary_detect.blob_size_hint");
            self.store.packed_first().blob_size_hint(&entry.id)?
        };
        self.cached_blob_sizes.insert(entry.id.clone(), size);
        Ok(size)
    }

    pub(crate) fn index_blob_binary_prefix(&mut self, entry: &IndexEntry) -> Result<bool> {
        if entry.mode == IndexMode::Gitlink {
            return Ok(false);
        }
        let Some(size) = self.index_blob_size_hint(entry)? else {
            return Ok(false);
        };
        if size <= BINARY_DETECTION_BYTES {
            self.ensure_index_blob(entry)?;
            return Ok(is_binary_content(self.index_blob_or_empty(entry)));
        }
        let Some(prefix) = ({
            let _trace =
                phase_trace("format_patch.write_tree_diff.entry_binary_detect.read_blob_prefix");
            self.store
                .packed_first()
                .read_blob_prefix(&entry.id, BINARY_DETECTION_BYTES)?
        }) else {
            return Ok(false);
        };
        Ok(is_binary_content(&prefix))
    }
}

pub(crate) fn render_diff(
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[skron_git_core::IndexDiffEntry],
    options: DiffRenderOptions,
) -> Result<()> {
    let context = DiffIndexContext {
        repo,
        store,
        old_index,
        new_index,
        old_source: options.old_source,
        new_source: options.new_source,
    };
    let stat_options = DiffStatOptions {
        whitespace_mode: options.whitespace_mode,
        relative_prefix: options.relative_prefix.as_deref(),
        ignore_matching_lines: &options.ignore_matching_lines,
    };
    let raw_options = RawPrintOptions {
        abbrev_len: options.raw_abbrev_len,
        relative_prefix: options.relative_prefix.as_deref(),
        nul_terminated: options.nul_terminated,
    };
    let has_diff = !entries.is_empty();
    if !options.quiet && !options.no_patch {
        if options.patch_with_stat {
            print_stat_entries(&context, entries, stat_options)?;
            if has_diff {
                println!();
            }
            print_render_patch_entries(repo, store, old_index, new_index, entries, &options)?;
        } else if options.patch_with_raw {
            print_raw_entries(&context, entries, raw_options)?;
            if has_diff {
                println!();
            }
            print_render_patch_entries(repo, store, old_index, new_index, entries, &options)?;
        } else if options.stat {
            print_stat_entries(&context, entries, stat_options)?;
        } else if options.numstat {
            print_numstat_entries(
                &context,
                entries,
                NumstatOptions {
                    stat: stat_options,
                    nul_terminated: options.nul_terminated,
                },
            )?;
        } else if options.shortstat {
            print_shortstat_entries(&context, entries, stat_options)?;
        } else if options.raw {
            print_raw_entries(&context, entries, raw_options)?;
        } else if options.summary {
            print_summary_entries(
                old_index,
                new_index,
                entries,
                options.relative_prefix.as_deref(),
            )?;
        } else if options.name_only {
            print_name_only_entries(
                entries,
                options.relative_prefix.as_deref(),
                options.nul_terminated,
            )?;
        } else if options.name_status {
            print_name_status_entries(
                entries,
                options.relative_prefix.as_deref(),
                options.nul_terminated,
            )?;
        } else if options.patch {
            print_render_patch_entries(repo, store, old_index, new_index, entries, &options)?;
        } else {
            print_raw_entries(&context, entries, raw_options)?;
        }
    }
    if has_diff && (options.quiet || options.exit_code) {
        return Err(CliError::Exit(1));
    }
    Ok(())
}

pub(crate) fn print_render_patch_entries(
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[skron_git_core::IndexDiffEntry],
    options: &DiffRenderOptions,
) -> Result<()> {
    let color_mode = if options.word_diff == WordDiffMode::Color {
        DiffColorMode::Always
    } else {
        options.color_mode
    };
    print_patch_entries(
        repo,
        store,
        old_index,
        new_index,
        entries,
        PatchFormatOptions {
            old_source: options.old_source,
            new_source: options.new_source,
            word_diff: options.word_diff,
            abbrev_len: options.patch_abbrev_len,
            old_prefix: options.old_prefix.clone(),
            new_prefix: options.new_prefix.clone(),
            unified_context: options.unified_context,
            inter_hunk_context: options.inter_hunk_context,
            output_indicator_new: options.output_indicator_new,
            output_indicator_old: options.output_indicator_old,
            output_indicator_context: options.output_indicator_context,
            ignore_matching_lines: options.ignore_matching_lines.clone(),
            whitespace_mode: options.whitespace_mode,
            relative_prefix: options.relative_prefix.clone(),
            text: options.text,
            binary: options.binary,
            irreversible_delete: options.irreversible_delete,
            submodule_format: options.submodule_format,
            color_mode,
            emit_hunk_headers: true,
        },
    )
}

pub(crate) fn diff_tree_root_entries(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    old: &str,
    new: Option<&str>,
    reverse: bool,
) -> Result<Vec<RootTreeDiffEntry>> {
    let (old_tree, new_tree) = if let Some(new) = new {
        (
            resolve_treeish(repo, store, old)?,
            resolve_treeish(repo, store, new)?,
        )
    } else {
        let id = resolve_objectish(repo, old).map_err(|_| ambiguous_revision_error(old))?;
        let commit = commit_cache.read_commit(&id)?;
        let old_tree = if let Some(parent) = commit.parents.first() {
            commit_cache.read_commit(parent)?.tree.clone()
        } else {
            zero_object_id()
        };
        (old_tree, commit.tree.clone())
    };
    let old_entries = if old_tree == zero_object_id() {
        Arc::<[skron_git_core::TreeEntry]>::from(Vec::new().into_boxed_slice())
    } else {
        tree_cache.read_tree(&old_tree)?
    };
    let new_entries = tree_cache.read_tree(&new_tree)?;
    let mut entries = diff_tree_root_entry_lists(&old_entries, &new_entries)?;
    if reverse {
        reverse_root_tree_diff_entries(&mut entries);
    }
    Ok(entries)
}

pub(crate) fn reverse_root_tree_diff_entries(entries: &mut [RootTreeDiffEntry]) {
    for entry in entries {
        entry.status = match entry.status {
            IndexDiffStatus::Added => IndexDiffStatus::Deleted,
            IndexDiffStatus::Deleted => IndexDiffStatus::Added,
            status => status,
        };
        std::mem::swap(&mut entry.old_mode, &mut entry.new_mode);
        std::mem::swap(&mut entry.old_id, &mut entry.new_id);
    }
}

pub(crate) fn diff_tree_root_entry_lists(
    old: &[TreeEntry],
    new: &[TreeEntry],
) -> Result<Vec<RootTreeDiffEntry>> {
    let mut paths = BTreeSet::new();
    for entry in old {
        paths.insert(entry.name.clone());
    }
    for entry in new {
        paths.insert(entry.name.clone());
    }
    let mut entries = Vec::new();
    for path in paths {
        let old_entry = old.iter().find(|entry| entry.name == path);
        let new_entry = new.iter().find(|entry| entry.name == path);
        if old_entry.map(|entry| (&entry.mode, &entry.id))
            == new_entry.map(|entry| (&entry.mode, &entry.id))
        {
            continue;
        }
        let status = match (old_entry, new_entry) {
            (None, Some(_)) => IndexDiffStatus::Added,
            (Some(_), None) => IndexDiffStatus::Deleted,
            (Some(_), Some(_)) => IndexDiffStatus::Modified,
            (None, None) => continue,
        };
        entries.push(RootTreeDiffEntry {
            status,
            path,
            old_mode: old_entry.map(|entry| entry.mode),
            new_mode: new_entry.map(|entry| entry.mode),
            old_id: old_entry.map(|entry| entry.id.clone()),
            new_id: new_entry.map(|entry| entry.id.clone()),
        });
    }
    Ok(entries)
}

pub(crate) fn render_diff_tree_root_entries(
    store: &LooseObjectStore,
    entries: Vec<RootTreeDiffEntry>,
    render: RootTreeRenderOptions<'_>,
) -> Result<()> {
    let RootTreeRenderOptions {
        diff_filter,
        order_file,
        skip_to,
        rotate_to,
        relative_prefix,
        options,
    } = render;
    let mut entries = apply_root_tree_diff_filter(entries, diff_filter);
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    if let Some(order_file) = order_file {
        let patterns = read_diff_order_patterns(order_file)?;
        entries.sort_by(|left, right| {
            root_tree_diff_order_sort_key(left, &patterns)
                .cmp(&root_tree_diff_order_sort_key(right, &patterns))
                .then_with(|| left.path.cmp(&right.path))
        });
    }
    entries = filter_root_tree_diff_relative(entries, relative_prefix);
    if let Some(target) = rotate_to
        && let Some(position) = entries
            .iter()
            .position(|entry| diff_display_path(&entry.path, relative_prefix) == target)
    {
        entries.rotate_left(position);
    }
    if let Some(target) = skip_to
        && let Some(position) = entries
            .iter()
            .position(|entry| diff_display_path(&entry.path, relative_prefix) == target)
    {
        entries.drain(..position);
    }
    if options.quiet {
        return if entries.is_empty() {
            Ok(())
        } else {
            Err(CliError::Exit(1))
        };
    }
    if options.name_only {
        print_root_tree_name_only_entries(&entries, relative_prefix, options.nul_terminated);
    } else if options.name_status {
        print_root_tree_name_status_entries(&entries, relative_prefix, options.nul_terminated);
    } else {
        print_root_tree_raw_entries(
            store,
            &entries,
            options.raw_abbrev_len,
            relative_prefix,
            options.nul_terminated,
        )?;
    }
    if options.exit_code && !entries.is_empty() {
        return Err(CliError::Exit(1));
    }
    Ok(())
}

pub(crate) fn apply_root_tree_diff_filter(
    entries: Vec<RootTreeDiffEntry>,
    diff_filter: DiffFilter,
) -> Vec<RootTreeDiffEntry> {
    entries
        .into_iter()
        .filter(|entry| diff_filter_matches(diff_filter, entry.status))
        .collect()
}

pub(crate) fn diff_filter_matches(filter: DiffFilter, status: IndexDiffStatus) -> bool {
    if filter.include_mask == 0 && filter.exclude_mask == 0 {
        return true;
    }
    let bit = diff_filter_status_bit(status);
    (filter.include_mask == 0 || filter.include_mask & bit != 0) && filter.exclude_mask & bit == 0
}

pub(crate) fn root_tree_diff_order_rank(
    entry: &RootTreeDiffEntry,
    patterns: &[String],
) -> Option<usize> {
    patterns
        .iter()
        .position(|pattern| diff_order_pattern_matches(pattern, &entry.path))
}

pub(crate) fn root_tree_diff_order_sort_key(
    entry: &RootTreeDiffEntry,
    patterns: &[String],
) -> (bool, usize) {
    match root_tree_diff_order_rank(entry, patterns) {
        Some(rank) => (false, rank),
        None => (true, usize::MAX),
    }
}

pub(crate) fn filter_root_tree_diff_relative(
    entries: Vec<RootTreeDiffEntry>,
    relative_prefix: Option<&[u8]>,
) -> Vec<RootTreeDiffEntry> {
    let Some(prefix) = relative_prefix else {
        return entries;
    };
    entries
        .into_iter()
        .filter(|entry| strip_diff_relative_path(&entry.path, prefix).is_some())
        .collect()
}

pub(crate) fn print_root_tree_name_only_entries(
    entries: &[RootTreeDiffEntry],
    relative_prefix: Option<&[u8]>,
    nul_terminated: bool,
) {
    for entry in entries {
        if nul_terminated {
            print!("{}\0", diff_display_path(&entry.path, relative_prefix));
        } else {
            println!("{}", diff_display_path(&entry.path, relative_prefix));
        }
    }
}

pub(crate) fn print_root_tree_name_status_entries(
    entries: &[RootTreeDiffEntry],
    relative_prefix: Option<&[u8]>,
    nul_terminated: bool,
) {
    for entry in entries {
        if nul_terminated {
            print!(
                "{}\0{}\0",
                entry.status.name_status(),
                diff_display_path(&entry.path, relative_prefix)
            );
        } else {
            println!(
                "{}\t{}",
                entry.status.name_status(),
                diff_display_path(&entry.path, relative_prefix)
            );
        }
    }
}

pub(crate) fn print_root_tree_raw_entries(
    store: &LooseObjectStore,
    entries: &[RootTreeDiffEntry],
    abbrev_len: Option<usize>,
    relative_prefix: Option<&[u8]>,
    nul_terminated: bool,
) -> Result<()> {
    let abbrev_len = abbrev_len.unwrap_or(default_abbrev_len(store)?);
    for entry in entries {
        let old_mode = entry.old_mode.map(tree_mode_octal).unwrap_or("000000");
        let new_mode = entry.new_mode.map(tree_mode_octal).unwrap_or("000000");
        let old_id = entry
            .old_id
            .as_ref()
            .map(|id| short_object_id_len(id, abbrev_len))
            .unwrap_or_else(|| short_zero_object_id_len(abbrev_len));
        let new_id = entry
            .new_id
            .as_ref()
            .map(|id| short_object_id_len(id, abbrev_len))
            .unwrap_or_else(|| short_zero_object_id_len(abbrev_len));
        if nul_terminated {
            print!(
                ":{old_mode} {new_mode} {old_id} {new_id} {}\0{}\0",
                entry.status.name_status(),
                diff_display_path(&entry.path, relative_prefix)
            );
        } else {
            println!(
                ":{old_mode} {new_mode} {old_id} {new_id} {}\t{}",
                entry.status.name_status(),
                diff_display_path(&entry.path, relative_prefix)
            );
        }
    }
    Ok(())
}

pub(crate) fn tree_mode_octal(mode: TreeMode) -> &'static str {
    match mode {
        TreeMode::Tree => "040000",
        TreeMode::File => "100644",
        TreeMode::Executable => "100755",
        TreeMode::Symlink => "120000",
        TreeMode::Gitlink => "160000",
    }
}

pub(crate) fn diff_tree_needs_recursive_entries(options: &PlumbingDiffOptions) -> bool {
    options.recursive
        || options.patch
        || options.patch_with_raw
        || options.patch_with_stat
        || options.binary
        || options.stat
        || options.numstat
        || options.shortstat
        || options.word_diff.is_some()
        || options.unified.is_some()
        || options.inter_hunk_context.is_some()
        || options.pickaxe_string.is_some()
        || options.pickaxe_regex.is_some()
        || options.pickaxe_all
        || options.pickaxe_regex_mode
}

pub(crate) struct DiffPairsBatch {
    pub(crate) old_index: GitIndex,
    pub(crate) new_index: GitIndex,
    pub(crate) entries: Vec<skron_git_core::IndexDiffEntry>,
}

pub(crate) fn parse_diff_pairs_batches(
    input: &[u8],
    nul_terminated: bool,
) -> Result<Vec<DiffPairsBatch>> {
    if !nul_terminated {
        return parse_diff_pairs_line_batches(input);
    }
    let mut batches = Vec::new();
    let mut old_index = GitIndex::new();
    let mut new_index = GitIndex::new();
    let mut entries = Vec::new();
    let mut fields = input.split(|byte| *byte == 0);
    while let Some(header) = fields.next() {
        if header.is_empty() {
            if !entries.is_empty() {
                batches.push(DiffPairsBatch {
                    old_index,
                    new_index,
                    entries,
                });
                old_index = GitIndex::new();
                new_index = GitIndex::new();
                entries = Vec::new();
            }
            continue;
        }
        let first_path = fields.next().ok_or_else(diff_pairs_bad_input)?.to_vec();
        let raw = parse_diff_pairs_raw_header(header)?;
        let path = if matches!(
            raw.status,
            IndexDiffStatus::Renamed | IndexDiffStatus::Copied
        ) {
            fields.next().ok_or_else(diff_pairs_bad_input)?.to_vec()
        } else {
            first_path.clone()
        };
        push_diff_pairs_entry(
            &mut old_index,
            &mut new_index,
            &mut entries,
            raw,
            first_path,
            path,
        )?;
    }
    if !entries.is_empty() {
        batches.push(DiffPairsBatch {
            old_index,
            new_index,
            entries,
        });
    }
    Ok(batches)
}

pub(crate) fn parse_diff_pairs_line_batches(input: &[u8]) -> Result<Vec<DiffPairsBatch>> {
    let mut batches = Vec::new();
    let mut old_index = GitIndex::new();
    let mut new_index = GitIndex::new();
    let mut entries = Vec::new();
    for line in input.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            if !entries.is_empty() {
                batches.push(DiffPairsBatch {
                    old_index,
                    new_index,
                    entries,
                });
                old_index = GitIndex::new();
                new_index = GitIndex::new();
                entries = Vec::new();
            }
            continue;
        }
        let mut fields = line.split(|byte| *byte == b'\t');
        let Some(header) = fields.next() else {
            return Err(diff_pairs_bad_input());
        };
        let first_path = fields.next().ok_or_else(diff_pairs_bad_input)?.to_vec();
        let second_path = fields.next();
        if fields.next().is_some() {
            return Err(diff_pairs_bad_input());
        }
        let raw = parse_diff_pairs_raw_header(header)?;
        let path = if matches!(
            raw.status,
            IndexDiffStatus::Renamed | IndexDiffStatus::Copied
        ) {
            let new_path = second_path.ok_or_else(diff_pairs_bad_input)?;
            new_path.to_vec()
        } else {
            if second_path.is_some() {
                return Err(diff_pairs_bad_input());
            }
            first_path.clone()
        };
        push_diff_pairs_entry(
            &mut old_index,
            &mut new_index,
            &mut entries,
            raw,
            first_path,
            path,
        )?;
    }
    if !entries.is_empty() {
        batches.push(DiffPairsBatch {
            old_index,
            new_index,
            entries,
        });
    }
    Ok(batches)
}

pub(crate) fn push_diff_pairs_entry(
    old_index: &mut GitIndex,
    new_index: &mut GitIndex,
    entries: &mut Vec<skron_git_core::IndexDiffEntry>,
    raw: DiffPairsRawHeader,
    first_path: Vec<u8>,
    path: Vec<u8>,
) -> Result<()> {
    if raw.old_mode.is_some() {
        old_index.upsert(diff_pairs_index_entry(
            first_path.clone(),
            raw.old_id,
            raw.old_mode,
        )?)?;
    }
    if raw.new_mode.is_some() {
        new_index.upsert(diff_pairs_index_entry(
            path.clone(),
            raw.new_id,
            raw.new_mode,
        )?)?;
    }
    entries.push(skron_git_core::IndexDiffEntry {
        status: raw.status,
        path,
        old_path: if matches!(
            raw.status,
            IndexDiffStatus::Renamed | IndexDiffStatus::Copied
        ) {
            Some(first_path)
        } else {
            None
        },
        similarity: raw.similarity,
    });
    Ok(())
}

pub(crate) struct DiffPairsRawHeader {
    old_mode: Option<IndexMode>,
    new_mode: Option<IndexMode>,
    old_id: ObjectId,
    new_id: ObjectId,
    status: IndexDiffStatus,
    similarity: Option<u8>,
}

pub(crate) fn parse_diff_pairs_raw_header(header: &[u8]) -> Result<DiffPairsRawHeader> {
    let header = std::str::from_utf8(header).map_err(|_| diff_pairs_bad_input())?;
    let header = header.strip_prefix(':').ok_or_else(diff_pairs_bad_input)?;
    let mut parts = header.split_whitespace();
    let old_mode = parse_diff_pairs_mode(parts.next().ok_or_else(diff_pairs_bad_input)?)?;
    let new_mode = parse_diff_pairs_mode(parts.next().ok_or_else(diff_pairs_bad_input)?)?;
    let old_id = parse_diff_pairs_id(parts.next().ok_or_else(diff_pairs_bad_input)?)?;
    let new_id = parse_diff_pairs_id(parts.next().ok_or_else(diff_pairs_bad_input)?)?;
    let (status, similarity) =
        parse_diff_pairs_status(parts.next().ok_or_else(diff_pairs_bad_input)?)?;
    if parts.next().is_some() {
        return Err(diff_pairs_bad_input());
    }
    Ok(DiffPairsRawHeader {
        old_mode,
        new_mode,
        old_id,
        new_id,
        status,
        similarity,
    })
}

pub(crate) fn parse_diff_pairs_mode(mode: &str) -> Result<Option<IndexMode>> {
    if mode == "000000" {
        Ok(None)
    } else {
        parse_index_mode(mode).map(Some)
    }
}

pub(crate) fn parse_diff_pairs_id(id: &str) -> Result<ObjectId> {
    if id.chars().all(|ch| ch == '0') {
        Ok(zero_object_id())
    } else {
        ObjectId::from_hex(GitHashAlgorithm::Sha1, id).map_err(CliError::Io)
    }
}

pub(crate) fn parse_diff_pairs_status(status: &str) -> Result<(IndexDiffStatus, Option<u8>)> {
    let similarity = status
        .get(1..)
        .filter(|value| !value.is_empty())
        .map(|value| value.parse::<u8>())
        .transpose()
        .map_err(|_| diff_pairs_bad_input())?;
    match status.as_bytes().first().copied() {
        Some(b'A') => Ok((IndexDiffStatus::Added, None)),
        Some(b'C') => Ok((IndexDiffStatus::Copied, similarity)),
        Some(b'D') => Ok((IndexDiffStatus::Deleted, None)),
        Some(b'M') => Ok((IndexDiffStatus::Modified, None)),
        Some(b'R') => Ok((IndexDiffStatus::Renamed, similarity)),
        _ => Err(diff_pairs_bad_input()),
    }
}

pub(crate) fn diff_pairs_index_entry(
    path: Vec<u8>,
    id: ObjectId,
    mode: Option<IndexMode>,
) -> Result<IndexEntry> {
    let mode = mode.ok_or_else(diff_pairs_bad_input)?;
    IndexEntry::new(path, id, mode, 0).map_err(CliError::Io)
}

pub(crate) fn diff_pairs_bad_input() -> CliError {
    CliError::Fatal {
        code: 128,
        message: "diff-pairs input is not valid NUL-terminated raw diff data".into(),
    }
}

pub(crate) fn filtered_diff_entries(
    repo: &GitRepo,
    old_index: &GitIndex,
    new_index: &GitIndex,
    paths: &[PathBuf],
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
    find_copies_harder: bool,
) -> Result<Vec<skron_git_core::IndexDiffEntry>> {
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative(repo, path))
        .collect::<Result<Vec<_>>>()?;
    Ok(diff_entries_for_indexes(
        old_index,
        new_index,
        detect_renames,
        detect_copies,
        find_copies_harder,
    )?
    .into_iter()
    .filter(|entry| diff_entry_matches_pathspec(entry, &pathspecs))
    .collect())
}

pub(crate) fn plumbing_render_options(options: &PlumbingDiffOptions) -> Result<DiffRenderOptions> {
    let explicit_abbrev_len = parse_diff_abbrev_len(options.abbrev.as_deref(), options.no_abbrev)?;
    let raw_abbrev_len = explicit_abbrev_len.or(Some(GitHashAlgorithm::Sha1.digest_len() * 2));
    let patch_abbrev_len = if options.full_index && !options.no_full_index {
        Some(GitHashAlgorithm::Sha1.digest_len() * 2)
    } else {
        explicit_abbrev_len
    };
    let (old_prefix, new_prefix) = diff_prefixes(
        options.no_prefix,
        options.default_prefix,
        options.src_prefix.clone(),
        options.dst_prefix.clone(),
    );
    let unified_context = options
        .unified
        .as_deref()
        .map(|value| parse_diff_context_value("--unified", value))
        .transpose()?
        .unwrap_or(3);
    let inter_hunk_context = options
        .inter_hunk_context
        .as_deref()
        .map(|value| parse_diff_context_value("--inter-hunk-context", value))
        .transpose()?
        .unwrap_or(0);
    let submodule_format = parse_submodule_diff_format(options.submodule.as_deref())?;
    validate_diff_algorithm_options(
        options.minimal,
        options.patience,
        options.histogram,
        options.diff_algorithm.as_deref(),
        &options.anchored,
    )?;
    let output_indicator_new = parse_output_indicator(
        "--output-indicator-new",
        options.output_indicator_new.as_deref(),
    )?;
    let output_indicator_old = parse_output_indicator(
        "--output-indicator-old",
        options.output_indicator_old.as_deref(),
    )?;
    let output_indicator_context = parse_output_indicator(
        "--output-indicator-context",
        options.output_indicator_context.as_deref(),
    )?;
    let ignore_matching_lines = compile_ignore_matching_lines(&options.ignore_matching_lines)?;
    let whitespace_mode = diff_whitespace_mode(
        options.ignore_space_at_eol,
        options.ignore_cr_at_eol,
        options.ignore_space_change,
        options.ignore_all_space,
        options.ignore_blank_lines,
    );
    let color_mode = parse_diff_color_option(options.color.as_deref(), options.no_color)?;
    let _accepted_noops = (
        options.no_ext_diff,
        options.no_textconv,
        options.no_color,
        options.no_color_moved,
        options.no_color_moved_ws,
    );
    Ok(DiffRenderOptions {
        stat: options.stat,
        patch_with_raw: options.patch_with_raw,
        patch_with_stat: options.patch_with_stat,
        numstat: options.numstat,
        shortstat: options.shortstat,
        raw: options.raw,
        summary: options.summary,
        name_status: options.name_status,
        name_only: options.name_only,
        nul_terminated: options.nul_terminated,
        patch: options.patch
            || options.binary
            || options.unified.is_some()
            || options.inter_hunk_context.is_some(),
        no_patch: options.no_patch,
        binary: options.binary,
        quiet: options.quiet,
        exit_code: options.quiet || options.exit_code,
        raw_abbrev_len,
        word_diff: WordDiffMode::None,
        patch_abbrev_len,
        old_prefix,
        new_prefix,
        unified_context,
        inter_hunk_context,
        output_indicator_new,
        output_indicator_old,
        output_indicator_context,
        ignore_matching_lines,
        whitespace_mode,
        relative_prefix: None,
        text: options.text,
        irreversible_delete: options.irreversible_delete,
        submodule_format,
        color_mode,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    })
}

pub(crate) struct DiffInput {
    pub(crate) old_index: GitIndex,
    pub(crate) new_index: GitIndex,
    pub(crate) new_side_from_index: bool,
    pub(crate) paths: Vec<PathBuf>,
}

pub(crate) fn parse_diff_input(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    cached: bool,
    args: Vec<PathBuf>,
) -> Result<DiffInput> {
    let (revs, paths) = split_diff_revs_and_paths(repo, store, args)?;
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    match (cached, revs.as_slice()) {
        (true, []) => Ok(DiffInput {
            old_index: read_head_index_with_caches(repo, &commit_cache, &tree_cache)?,
            new_index: index.clone(),
            new_side_from_index: true,
            paths,
        }),
        (true, [old]) => Ok(DiffInput {
            old_index: read_treeish_index_cached(repo, store, &tree_cache, old)?,
            new_index: index.clone(),
            new_side_from_index: true,
            paths,
        }),
        (true, [_, _, ..]) => Err(CliError::Fatal {
            code: 129,
            message: "`diff --cached` accepts at most one commit".into(),
        }),
        (false, []) => Ok(DiffInput {
            old_index: index.clone(),
            new_index: worktree_index_snapshot(repo, index)?,
            new_side_from_index: false,
            paths,
        }),
        (false, [old]) => Ok(DiffInput {
            old_index: read_treeish_index_cached(repo, store, &tree_cache, old)?,
            new_index: worktree_index_snapshot(repo, index)?,
            new_side_from_index: false,
            paths,
        }),
        (false, [old, new]) => Ok(DiffInput {
            old_index: read_treeish_index_cached(repo, store, &tree_cache, old)?,
            new_index: read_treeish_index_cached(repo, store, &tree_cache, new)?,
            new_side_from_index: true,
            paths,
        }),
        (false, [_, _, ..]) => Err(CliError::Fatal {
            code: 129,
            message: "`diff` accepts at most two commits".into(),
        }),
    }
}

pub(crate) fn split_diff_revs_and_paths(
    repo: &GitRepo,
    store: &LooseObjectStore,
    args: Vec<PathBuf>,
) -> Result<(Vec<String>, Vec<PathBuf>)> {
    let mut revs = Vec::new();
    let mut path_start = 0;
    for (idx, arg) in args.iter().enumerate() {
        if revs.len() == 2 {
            path_start = idx;
            break;
        }
        let arg = arg.to_string_lossy();
        if arg == "--" {
            path_start = idx + 1;
            break;
        }
        if resolve_treeish(repo, store, &arg).is_ok() {
            revs.push(arg.into_owned());
            path_start = idx + 1;
        } else {
            if !repo.root.join(std::path::Path::new(arg.as_ref())).exists() {
                return Err(ambiguous_revision_error(&arg));
            }
            path_start = idx;
            break;
        }
    }
    Ok((revs, args.into_iter().skip(path_start).collect()))
}

pub(crate) fn read_treeish_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    treeish: &str,
) -> Result<GitIndex> {
    let tree_cache = TreeObjectCache::new(store);
    read_treeish_index_cached(repo, store, &tree_cache, treeish)
}

pub(crate) fn read_treeish_index_cached(
    repo: &GitRepo,
    store: &LooseObjectStore,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    treeish: &str,
) -> Result<GitIndex> {
    let tree = resolve_treeish(repo, store, treeish)?;
    Ok(tree_cache.read_tree_to_index(&tree)?)
}

pub(crate) fn read_commit_tree_index_cached<S: GitObjectStore + ?Sized>(
    tree_cache: &TreeObjectCache<'_, S>,
    commit: &CommitObject,
) -> Result<GitIndex> {
    Ok(tree_cache.read_tree_to_index(&commit.tree)?)
}

pub(crate) fn parse_stash_show_abbrev(value: &str) -> Result<usize> {
    value
        .parse::<usize>()
        .map(|value| value.max(4))
        .map_err(|_| CliError::Fatal {
            code: 129,
            message: format!("invalid --abbrev value '{value}'"),
        })
}

pub(crate) fn parse_diff_abbrev_len(value: Option<&str>, no_abbrev: bool) -> Result<Option<usize>> {
    if no_abbrev {
        return Ok(Some(GitHashAlgorithm::Sha1.digest_len() * 2));
    }
    value
        .filter(|value| !value.is_empty())
        .map(parse_stash_show_abbrev)
        .transpose()
}

pub(crate) fn diff_prefixes(
    no_prefix: bool,
    default_prefix: bool,
    src_prefix: Option<String>,
    dst_prefix: Option<String>,
) -> (String, String) {
    let mut old_prefix = "a/".to_owned();
    let mut new_prefix = "b/".to_owned();
    if no_prefix && !default_prefix {
        old_prefix.clear();
        new_prefix.clear();
    }
    if let Some(prefix) = src_prefix {
        old_prefix = prefix;
    }
    if let Some(prefix) = dst_prefix {
        new_prefix = prefix;
    }
    (old_prefix, new_prefix)
}

pub(crate) fn diff_relative_prefix(
    repo: &GitRepo,
    value: Option<&str>,
    no_relative: bool,
) -> Result<Option<Vec<u8>>> {
    if no_relative {
        return Ok(None);
    }
    let Some(value) = value else {
        return Ok(None);
    };
    let mut prefix = if value.is_empty() {
        repo_relative_path(&repo.root, &std::env::current_dir()?)?
    } else {
        path_arg_to_repo_relative_allow_root(repo, Path::new(value))?
    };
    while prefix.ends_with(b"/") {
        prefix.pop();
    }
    if prefix.is_empty() {
        Ok(None)
    } else {
        Ok(Some(prefix))
    }
}

pub(crate) fn diff_whitespace_mode(
    ignore_space_at_eol: bool,
    ignore_cr_at_eol: bool,
    ignore_space_change: bool,
    ignore_all_space: bool,
    _ignore_blank_lines: bool,
) -> DiffWhitespaceMode {
    if ignore_all_space {
        DiffWhitespaceMode::All
    } else if ignore_space_change {
        DiffWhitespaceMode::Change
    } else if ignore_cr_at_eol {
        DiffWhitespaceMode::CrAtEol
    } else if ignore_space_at_eol {
        DiffWhitespaceMode::AtEol
    } else {
        DiffWhitespaceMode::None
    }
}

pub(crate) fn parse_diff_color_option(
    value: Option<&str>,
    no_color: bool,
) -> Result<DiffColorMode> {
    if no_color {
        return Ok(DiffColorMode::Never);
    }
    match value {
        None | Some("never") | Some("false") | Some("no") => Ok(DiffColorMode::Never),
        Some("") | Some("always") | Some("true") | Some("yes") => Ok(DiffColorMode::Always),
        Some("auto") => Ok(DiffColorMode::Auto),
        Some(other) => Err(CliError::Fatal {
            code: 129,
            message: format!("bad boolean config value '{other}' for 'diff.color'"),
        }),
    }
}

pub(crate) fn filter_diff_relative(
    entries: Vec<skron_git_core::IndexDiffEntry>,
    prefix: Option<&[u8]>,
) -> Vec<skron_git_core::IndexDiffEntry> {
    let Some(prefix) = prefix else {
        return entries;
    };
    entries
        .into_iter()
        .filter(|entry| {
            strip_diff_relative_path(&entry.path, prefix).is_some()
                || strip_diff_relative_path(diff_entry_old_path(entry), prefix).is_some()
        })
        .collect()
}

pub(crate) fn strip_diff_relative_path<'a, 'b>(
    path: &'a [u8],
    prefix: &'b [u8],
) -> Option<&'a [u8]> {
    let rest = path.strip_prefix(prefix)?;
    if rest.is_empty() {
        return None;
    }
    rest.strip_prefix(b"/")
}

pub(crate) fn parse_diff_context_value(option: &str, value: &str) -> Result<usize> {
    value.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 129,
        message: format!("invalid {option} value '{value}'"),
    })
}

pub(crate) fn parse_output_indicator(option: &str, value: Option<&str>) -> Result<Option<u8>> {
    let Some(value) = value else {
        return Ok(match option {
            "--output-indicator-new" => Some(b'+'),
            "--output-indicator-old" => Some(b'-'),
            "--output-indicator-context" => Some(b' '),
            _ => None,
        });
    };
    match value.as_bytes() {
        [] => Ok(None),
        [byte] => Ok(Some(*byte)),
        _ => Err(CliError::Fatal {
            code: 129,
            message: format!("{option} expects a character, got '{value}'"),
        }),
    }
}

pub(crate) fn compile_ignore_matching_lines(patterns: &[String]) -> Result<Vec<Regex>> {
    patterns
        .iter()
        .map(|pattern| {
            Regex::new(pattern).map_err(|error| CliError::Fatal {
                code: 129,
                message: format!("invalid ignore-matching-lines regex '{pattern}': {error}"),
            })
        })
        .collect()
}

#[derive(Clone, Copy, Default)]
pub(crate) struct DiffFilter {
    include_mask: u16,
    exclude_mask: u16,
}

pub(crate) fn parse_diff_filter(value: &str) -> Result<DiffFilter> {
    let mut filter = DiffFilter::default();
    for byte in value.bytes() {
        let bit = diff_filter_bit(byte.to_ascii_lowercase()).ok_or_else(|| CliError::Fatal {
            code: 129,
            message: format!("unsupported --diff-filter status '{}'", byte as char),
        })?;
        if byte.is_ascii_lowercase() {
            filter.exclude_mask |= bit;
        } else {
            filter.include_mask |= bit;
        }
    }
    Ok(filter)
}

pub(crate) fn diff_filter_bit(status: u8) -> Option<u16> {
    match status {
        b'a' => Some(1 << 0),
        b'c' => Some(1 << 1),
        b'd' => Some(1 << 2),
        b'm' => Some(1 << 3),
        b'r' => Some(1 << 4),
        b't' => Some(1 << 5),
        b'u' => Some(1 << 6),
        b'x' => Some(1 << 7),
        b'b' => Some(1 << 8),
        _ => None,
    }
}

pub(crate) fn apply_diff_filter(
    entries: Vec<skron_git_core::IndexDiffEntry>,
    filter: DiffFilter,
) -> Vec<skron_git_core::IndexDiffEntry> {
    if filter.include_mask == 0 && filter.exclude_mask == 0 {
        return entries;
    }
    entries
        .into_iter()
        .filter(|entry| {
            let bit = diff_filter_status_bit(entry.status);
            (filter.include_mask == 0 || filter.include_mask & bit != 0)
                && filter.exclude_mask & bit == 0
        })
        .collect()
}

pub(crate) fn diff_filter_status_bit(status: IndexDiffStatus) -> u16 {
    match status {
        IndexDiffStatus::Added => 1 << 0,
        IndexDiffStatus::Copied => 1 << 1,
        IndexDiffStatus::Deleted => 1 << 2,
        IndexDiffStatus::Modified => 1 << 3,
        IndexDiffStatus::Renamed => 1 << 4,
    }
}

pub(crate) fn config_bool_enabled(repo: &GitRepo, name: &str) -> Result<bool> {
    Ok(read_config_value(repo, name)?
        .as_deref()
        .is_some_and(config_bool_value_enabled))
}

pub(crate) fn config_bool_value_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "on" | "1"
    )
}

pub(crate) fn write_format_patch_with_tree_diff_cached<W: Write, S: GitObjectStore + ?Sized>(
    out: &mut W,
    context: &FormatPatchContext<'_>,
    entry: FormatPatchEntry<'_>,
    tree_cache: &TreeObjectCache<'_, S>,
    old_tree: Option<&ObjectId>,
    new_tree: &ObjectId,
    blob_cache: &mut FormatPatchBlobCache<'_>,
) -> Result<()> {
    let FormatPatchEntry { id, commit, number } = entry;
    {
        let _trace = phase_trace("format_patch.write_header");
        write_format_patch_header(out, context, id, commit, number)?;
    }
    {
        let _trace = phase_trace("format_patch.write_tree_diff");
        write_commit_patch_entries_tree_diff_cached(
            out,
            context.repo,
            context.store,
            tree_cache,
            old_tree,
            new_tree,
            context.abbrev_len,
            blob_cache,
        )?;
    }
    writeln!(out, "-- ")?;
    writeln!(out, "skron-git")?;
    Ok(())
}

fn write_format_patch_header<W: Write>(
    out: &mut W,
    context: &FormatPatchContext<'_>,
    id: &ObjectId,
    commit: &CommitObject,
    number: usize,
) -> Result<()> {
    let FormatPatchContext { total, .. } = context;
    let subject = commit_subject_view(&commit.message);
    write!(out, "From ")?;
    id.write_hex_io(out)?;
    writeln!(out, " Mon Sep 17 00:00:00 2001")?;
    writeln!(
        out,
        "From: {} <{}>",
        signature_name(&commit.author),
        signature_email(&commit.author)
    )?;
    writeln!(out, "Date: {}", signature_mail_date(&commit.author)?)?;
    if *total > 1 {
        writeln!(out, "Subject: [PATCH {number}/{total}] {subject}")?;
    } else {
        writeln!(out, "Subject: [PATCH] {subject}")?;
    }
    writeln!(out)?;
    write_commit_message_body(out, &commit.message)?;
    writeln!(out, "---")?;
    Ok(())
}

pub(crate) fn write_commit_patch_entries_tree_diff_cached<W: Write, S: GitObjectStore + ?Sized>(
    out: &mut W,
    repo: &GitRepo,
    store: &LooseObjectStore,
    tree_cache: &TreeObjectCache<'_, S>,
    old_tree: Option<&ObjectId>,
    new_tree: &ObjectId,
    abbrev_len: usize,
    blob_cache: &mut FormatPatchBlobCache<'_>,
) -> Result<()> {
    write_patch_entries_streaming_from_tree_diff(
        out,
        tree_cache,
        old_tree,
        new_tree,
        cached_patch_write_context(repo, store, abbrev_len),
        WordDiffMode::None,
        blob_cache,
    )
}

fn cached_patch_write_context<'a>(
    repo: &'a GitRepo,
    store: &'a LooseObjectStore,
    abbrev_len: usize,
) -> PatchWriteContext<'a> {
    PatchWriteContext {
        repo,
        store,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
        abbrev_len,
        old_prefix: "a/",
        new_prefix: "b/",
        unified_context: 3,
        inter_hunk_context: 0,
        output_indicator_new: Some(b'+'),
        output_indicator_old: Some(b'-'),
        output_indicator_context: Some(b' '),
        ignore_matching_lines: &[],
        whitespace_mode: DiffWhitespaceMode::None,
        relative_prefix: None,
        text: false,
        binary: true,
        irreversible_delete: false,
        submodule_format: SubmoduleDiffFormat::Short,
        color: false,
        emit_hunk_headers: true,
    }
}

#[cfg(test)]
pub(crate) fn commit_message_body(message: &[u8]) -> String {
    let body = commit_message_body_view(message);
    if let Ok(body) = std::str::from_utf8(body) {
        body.to_owned()
    } else {
        String::from_utf8_lossy(body).into_owned()
    }
}

fn commit_subject_view(message: &[u8]) -> Cow<'_, str> {
    let mut end = 0;
    while end < message.len() {
        if message[end] == b'\n' {
            break;
        }
        end += 1;
    }
    let mut subject = &message[..end];
    if subject.ends_with(b"\r") {
        subject = &subject[..subject.len() - 1];
    }
    match std::str::from_utf8(subject) {
        Ok(message) => Cow::Borrowed(message),
        Err(_) => Cow::Owned(String::from_utf8_lossy(subject).into_owned()),
    }
}

fn commit_message_body_view(message: &[u8]) -> &[u8] {
    let mut end = message.len();
    while end > 0 && message[end - 1] == b'\n' {
        end -= 1;
    }

    let message = &message[..end];
    let mut body_start = 0_usize;
    let mut found_body = false;
    for idx in 0..message.len().saturating_sub(1) {
        if message[idx] == b'\n' && message[idx + 1] == b'\n' {
            body_start = idx + 2;
            found_body = true;
            break;
        }
    }
    if !found_body {
        return &[];
    }
    let mut body_end = message.len();
    while body_end > body_start && message[body_end - 1] == b'\n' {
        body_end -= 1;
    }
    if body_end <= body_start {
        &[]
    } else {
        &message[body_start..body_end]
    }
}

fn write_commit_message_body<W: Write>(out: &mut W, message: &[u8]) -> Result<()> {
    let body = commit_message_body_view(message);
    if body.is_empty() {
        return Ok(());
    }

    out.write_all(body)?;
    if !body.ends_with(b"\n") {
        writeln!(out)?;
    }
    writeln!(out)?;
    Ok(())
}

pub(crate) fn format_patch_filename(number: usize, subject: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for ch in subject.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            slug.push(ch);
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "patch" } else { slug };
    format!("{number:04}-{slug}.patch")
}

pub(crate) fn relative_path_between(
    from: &std::path::Path,
    to: &std::path::Path,
) -> Option<PathBuf> {
    let from_components = from.components().collect::<Vec<_>>();
    let to_components = to.components().collect::<Vec<_>>();
    if from_components.first() != to_components.first() {
        return None;
    }
    let common = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| left == right)
        .count();
    let mut out = PathBuf::new();
    for _ in common..from_components.len() {
        out.push("..");
    }
    for component in &to_components[common..] {
        out.push(component.as_os_str());
    }
    Some(out)
}

pub(crate) fn git_relative_display(repo: &GitRepo, path: &std::path::Path) -> Result<String> {
    let cwd = std::env::current_dir()?;
    match path.strip_prefix(&cwd) {
        Ok(relative) => Ok(relative.display().to_string()),
        Err(_) => match path.strip_prefix(&repo.root) {
            Ok(relative) => Ok(relative.display().to_string()),
            Err(_) => Ok(path.display().to_string()),
        },
    }
}

pub(crate) fn print_name_status_entries(
    entries: &[skron_git_core::IndexDiffEntry],
    relative_prefix: Option<&[u8]>,
    nul_terminated: bool,
) -> Result<()> {
    for entry in entries {
        if matches!(
            entry.status,
            IndexDiffStatus::Renamed | IndexDiffStatus::Copied
        ) {
            if nul_terminated {
                print!(
                    "{}\0{}\0{}\0",
                    diff_entry_status_name(entry),
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    diff_display_path(&entry.path, relative_prefix)
                );
            } else {
                println!(
                    "{}\t{}\t{}",
                    diff_entry_status_name(entry),
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    diff_display_path(&entry.path, relative_prefix)
                );
            }
        } else if nul_terminated {
            print!(
                "{}\0{}\0",
                diff_entry_status_name(entry),
                diff_display_path(&entry.path, relative_prefix)
            );
        } else {
            println!(
                "{}\t{}",
                diff_entry_status_name(entry),
                diff_display_path(&entry.path, relative_prefix)
            );
        }
    }
    Ok(())
}

pub(crate) fn diff_entry_status_name(entry: &skron_git_core::IndexDiffEntry) -> String {
    match entry.status {
        IndexDiffStatus::Renamed => format!("R{:03}", entry.similarity.unwrap_or(100)),
        IndexDiffStatus::Copied => format!("C{:03}", entry.similarity.unwrap_or(100)),
        _ => entry.status.name_status().to_owned(),
    }
}

pub(crate) fn print_name_only_entries(
    entries: &[skron_git_core::IndexDiffEntry],
    relative_prefix: Option<&[u8]>,
    nul_terminated: bool,
) -> Result<()> {
    for entry in entries {
        if nul_terminated {
            print!("{}\0", diff_display_path(&entry.path, relative_prefix));
        } else {
            println!("{}", diff_display_path(&entry.path, relative_prefix));
        }
    }
    Ok(())
}

pub(crate) fn diff_entry_old_path(entry: &skron_git_core::IndexDiffEntry) -> &[u8] {
    entry.old_path.as_deref().unwrap_or(&entry.path)
}

fn diff_display_path_owned<'a>(path: &'a [u8]) -> Cow<'a, str> {
    String::from_utf8_lossy(path)
}

pub(crate) fn diff_display_path<'a, 'b>(
    path: &'a [u8],
    relative_prefix: Option<&'b [u8]>,
) -> Cow<'a, str> {
    if let Some(prefix) = relative_prefix
        && let Some(stripped) = strip_diff_relative_path(path, prefix)
    {
        return diff_display_path_owned(stripped);
    }
    diff_display_path_owned(path)
}

pub(crate) fn diff_entry_stat_path<'a, 'b>(
    entry: &'a skron_git_core::IndexDiffEntry,
    relative_prefix: Option<&'b [u8]>,
) -> Cow<'a, str> {
    if matches!(
        entry.status,
        IndexDiffStatus::Renamed | IndexDiffStatus::Copied
    ) {
        Cow::Owned(format!(
            "{} => {}",
            diff_display_path(diff_entry_old_path(entry), relative_prefix),
            diff_display_path(&entry.path, relative_prefix)
        ))
    } else {
        diff_display_path(&entry.path, relative_prefix)
    }
}

pub(crate) fn print_raw_entries(
    context: &DiffIndexContext<'_>,
    entries: &[skron_git_core::IndexDiffEntry],
    options: RawPrintOptions<'_>,
) -> Result<()> {
    let RawPrintOptions {
        abbrev_len,
        relative_prefix,
        nul_terminated,
    } = options;
    let abbrev_len = abbrev_len.unwrap_or(default_abbrev_len(context.store)?);
    for entry in entries {
        let old_entry = find_index_entry(context.old_index, diff_entry_old_path(entry));
        let new_entry = find_index_entry(context.new_index, &entry.path);
        let old_mode = old_entry
            .map(|entry| index_mode_octal(entry.mode))
            .unwrap_or("000000");
        let new_mode = new_entry
            .map(|entry| index_mode_octal(entry.mode))
            .unwrap_or("000000");
        let old_id = context.old_source.raw_id(old_entry, abbrev_len);
        let new_id = context.new_source.raw_id(new_entry, abbrev_len);
        if matches!(
            entry.status,
            IndexDiffStatus::Renamed | IndexDiffStatus::Copied
        ) {
            if nul_terminated {
                print!(
                    ":{old_mode} {new_mode} {old_id} {new_id} {}\0{}\0{}\0",
                    diff_entry_status_name(entry),
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    diff_display_path(&entry.path, relative_prefix)
                );
            } else {
                println!(
                    ":{old_mode} {new_mode} {old_id} {new_id} {}\t{}\t{}",
                    diff_entry_status_name(entry),
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    diff_display_path(&entry.path, relative_prefix)
                );
            }
        } else if nul_terminated {
            print!(
                ":{old_mode} {new_mode} {old_id} {new_id} {}\0{}\0",
                diff_entry_status_name(entry),
                diff_display_path(&entry.path, relative_prefix)
            );
        } else {
            println!(
                ":{old_mode} {new_mode} {old_id} {new_id} {}\t{}",
                diff_entry_status_name(entry),
                diff_display_path(&entry.path, relative_prefix)
            );
        }
    }
    Ok(())
}

pub(crate) fn short_zero_object_id_len(len: usize) -> String {
    "0".repeat(len)
}

pub(crate) fn print_summary_entries(
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[skron_git_core::IndexDiffEntry],
    relative_prefix: Option<&[u8]>,
) -> Result<()> {
    for entry in entries {
        let old_entry = find_index_entry(old_index, diff_entry_old_path(entry));
        let new_entry = find_index_entry(new_index, &entry.path);
        let path = diff_display_path(&entry.path, relative_prefix);
        match (entry.status, old_entry, new_entry) {
            (IndexDiffStatus::Copied, Some(_), Some(_)) => {
                println!(
                    " copy {} => {} (100%)",
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    path
                );
            }
            (IndexDiffStatus::Renamed, Some(_), Some(_)) => {
                println!(
                    " rename {} => {} (100%)",
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    path
                );
            }
            (IndexDiffStatus::Added, _, Some(new_entry)) => {
                println!(" create mode {} {path}", index_mode_octal(new_entry.mode));
            }
            (IndexDiffStatus::Deleted, Some(old_entry), _) => {
                println!(" delete mode {} {path}", index_mode_octal(old_entry.mode));
            }
            (IndexDiffStatus::Modified, Some(old_entry), Some(new_entry))
                if old_entry.mode != new_entry.mode =>
            {
                println!(
                    " mode change {} => {} {path}",
                    index_mode_octal(old_entry.mode),
                    index_mode_octal(new_entry.mode)
                );
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct DiffStatRow {
    path: String,
    old_bytes: usize,
    new_bytes: usize,
    insertions: usize,
    deletions: usize,
    binary: bool,
}

pub(crate) struct FormatPatchContext<'a> {
    pub(crate) repo: &'a GitRepo,
    pub(crate) store: &'a LooseObjectStore,
    pub(crate) abbrev_len: usize,
    pub(crate) total: usize,
}

pub(crate) struct FormatPatchEntry<'a> {
    pub(crate) id: &'a ObjectId,
    pub(crate) commit: &'a skron_git_core::CommitObject,
    pub(crate) number: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct RawPrintOptions<'a> {
    pub(crate) abbrev_len: Option<usize>,
    pub(crate) relative_prefix: Option<&'a [u8]>,
    pub(crate) nul_terminated: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct DiffStatOptions<'a> {
    pub(crate) whitespace_mode: DiffWhitespaceMode,
    pub(crate) relative_prefix: Option<&'a [u8]>,
    pub(crate) ignore_matching_lines: &'a [Regex],
}

#[derive(Clone, Copy)]
pub(crate) struct NumstatOptions<'a> {
    pub(crate) stat: DiffStatOptions<'a>,
    pub(crate) nul_terminated: bool,
}

pub(crate) fn print_stat_entries(
    context: &DiffIndexContext<'_>,
    entries: &[skron_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
) -> Result<()> {
    print_stat_entries_with_whitespace(context, entries, options)
}

pub(crate) fn print_stat_entries_with_whitespace(
    context: &DiffIndexContext<'_>,
    entries: &[skron_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
) -> Result<()> {
    let rows = diff_stat_rows_with_whitespace(context, entries, options)?;
    if rows.is_empty() {
        return Ok(());
    }
    let max_width = 79usize;
    let has_binary_rows = rows.iter().any(|row| row.binary);
    let change_width = rows
        .iter()
        .filter(|row| !row.binary)
        .map(|row| row.insertions + row.deletions)
        .max()
        .unwrap_or(0)
        .to_string()
        .len()
        .max(if has_binary_rows { 3 } else { 0 });
    let path_width = rows
        .iter()
        .map(|row| row.path.len())
        .max()
        .unwrap_or(0)
        .min(50);
    let graph_width = max_width
        .saturating_sub(1 + path_width + 3 + change_width + 1)
        .max(1);
    let max_changes = rows
        .iter()
        .filter(|row| !row.binary)
        .map(|row| row.insertions + row.deletions)
        .max()
        .unwrap_or(0);
    for row in &rows {
        let path = compact_stat_path(&row.path, path_width);
        let path_padding = " ".repeat(path_width.saturating_sub(path.len()));
        if row.binary {
            println!(
                " {}{} | {:>change_width$} {} -> {} bytes",
                path, path_padding, "Bin", row.old_bytes, row.new_bytes
            );
            continue;
        }
        let changes = row.insertions + row.deletions;
        let graph = stat_graph(row.insertions, row.deletions, max_changes, graph_width);
        if graph.is_empty() {
            println!(" {}{} | {:>change_width$}", path, path_padding, changes);
        } else {
            println!(
                " {}{} | {:>change_width$} {}",
                path, path_padding, changes, graph
            );
        }
    }
    print_diff_stat_summary(&rows);
    Ok(())
}

pub(crate) fn compact_stat_path(path: &str, width: usize) -> String {
    if path.len() <= width {
        return path.to_owned();
    }
    if width <= 3 {
        return path.chars().take(width).collect();
    }
    let mut tail = String::new();
    for part in path.rsplit('/') {
        let candidate_len = if tail.is_empty() {
            part.len()
        } else {
            part.len() + 1 + tail.len()
        };
        if candidate_len + 4 > width {
            break;
        }
        if tail.is_empty() {
            tail.push_str(part);
        } else {
            tail.insert(0, '/');
            tail.insert_str(0, part);
        }
    }
    if tail.is_empty() {
        format!("...{}", trailing_chars(path, width - 3))
    } else {
        format!(".../{tail}")
    }
}

fn trailing_chars(value: &str, count: usize) -> &str {
    if count == 0 {
        return "";
    }
    let Some((start, _)) = value.char_indices().rev().nth(count - 1) else {
        return value;
    };
    &value[start..]
}

pub(crate) fn stat_graph(
    insertions: usize,
    deletions: usize,
    max_changes: usize,
    graph_width: usize,
) -> String {
    let changes = insertions + deletions;
    if changes == 0 {
        return String::new();
    }
    let scaled_total = if max_changes > graph_width {
        (changes * graph_width).div_ceil(max_changes)
    } else {
        changes
    }
    .max(1)
    .min(graph_width);

    let mut plus = if insertions == 0 {
        0
    } else {
        (insertions * scaled_total).div_ceil(changes)
    };
    let mut minus = scaled_total.saturating_sub(plus);
    if insertions > 0 && plus == 0 {
        plus = 1;
    }
    if deletions > 0 && minus == 0 {
        minus = 1;
    }
    if plus + minus > graph_width {
        if plus >= minus {
            plus = plus.saturating_sub(plus + minus - graph_width);
        } else {
            minus = minus.saturating_sub(plus + minus - graph_width);
        }
    }
    format!("{}{}", "+".repeat(plus), "-".repeat(minus))
}

pub(crate) fn print_shortstat_entries(
    context: &DiffIndexContext<'_>,
    entries: &[skron_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
) -> Result<()> {
    let rows = diff_stat_rows_with_whitespace(context, entries, options)?;
    if !rows.is_empty() {
        print_diff_stat_summary(&rows);
    }
    Ok(())
}

pub(crate) fn print_numstat_entries(
    context: &DiffIndexContext<'_>,
    entries: &[skron_git_core::IndexDiffEntry],
    options: NumstatOptions<'_>,
) -> Result<()> {
    let NumstatOptions {
        stat:
            DiffStatOptions {
                whitespace_mode,
                relative_prefix,
                ignore_matching_lines,
            },
        nul_terminated,
    } = options;
    for entry in entries {
        let row = diff_stat_row_with_whitespace(
            context,
            entry,
            DiffStatOptions {
                whitespace_mode,
                relative_prefix,
                ignore_matching_lines,
            },
        )?;
        if (whitespace_mode != DiffWhitespaceMode::None || !ignore_matching_lines.is_empty())
            && !row.binary
            && row.insertions + row.deletions == 0
        {
            continue;
        }
        if row.binary {
            if nul_terminated
                && matches!(
                    entry.status,
                    IndexDiffStatus::Renamed | IndexDiffStatus::Copied
                )
            {
                print!(
                    "-\t-\t\0{}\0{}\0",
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    diff_display_path(&entry.path, relative_prefix)
                );
            } else if nul_terminated {
                print!("-\t-\t{}\0", row.path);
            } else {
                println!("-\t-\t{}", row.path);
            }
        } else if nul_terminated
            && matches!(
                entry.status,
                IndexDiffStatus::Renamed | IndexDiffStatus::Copied
            )
        {
            print!(
                "{}\t{}\t\0{}\0{}\0",
                row.insertions,
                row.deletions,
                diff_display_path(diff_entry_old_path(entry), relative_prefix),
                diff_display_path(&entry.path, relative_prefix)
            );
        } else if nul_terminated {
            print!("{}\t{}\t{}\0", row.insertions, row.deletions, row.path);
        } else {
            println!("{}\t{}\t{}", row.insertions, row.deletions, row.path);
        }
    }
    Ok(())
}

pub(crate) fn diff_stat_rows_with_whitespace(
    context: &DiffIndexContext<'_>,
    entries: &[skron_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
) -> Result<Vec<DiffStatRow>> {
    let mut rows = Vec::with_capacity(diff_stat_row_initial_capacity(entries.len()));
    for entry in entries {
        let row = diff_stat_row_with_whitespace(context, entry, options)?;
        if (options.whitespace_mode == DiffWhitespaceMode::None
            && options.ignore_matching_lines.is_empty())
            || row.binary
            || row.insertions + row.deletions > 0
        {
            rows.push(row);
        }
    }
    Ok(rows)
}

pub(crate) fn diff_stat_row_with_whitespace(
    context: &DiffIndexContext<'_>,
    entry: &skron_git_core::IndexDiffEntry,
    options: DiffStatOptions<'_>,
) -> Result<DiffStatRow> {
    let old_entry = find_index_entry(context.old_index, diff_entry_old_path(entry));
    let new_entry = find_index_entry(context.new_index, &entry.path);
    let old_content = old_entry
        .map(|entry| read_diff_side_content(context.repo, context.store, entry, context.old_source))
        .transpose()?
        .unwrap_or_default();
    let new_content = match new_entry {
        Some(entry) => {
            read_diff_side_content(context.repo, context.store, entry, context.new_source)?
        }
        None => Vec::new(),
    };
    let binary = is_binary_content(&old_content) || is_binary_content(&new_content);
    let (insertions, deletions) = if binary {
        (0, 0)
    } else {
        diff_line_counts_with_options(
            &old_content,
            &new_content,
            options.whitespace_mode,
            options.ignore_matching_lines,
        )
    };
    Ok(DiffStatRow {
        path: diff_entry_stat_path(entry, options.relative_prefix).into_owned(),
        old_bytes: old_content.len(),
        new_bytes: new_content.len(),
        insertions,
        deletions,
        binary,
    })
}

pub(crate) fn diff_line_counts_with_options(
    old: &[u8],
    new: &[u8],
    whitespace_mode: DiffWhitespaceMode,
    ignore_matching_lines: &[Regex],
) -> (usize, usize) {
    let old_lines = split_diff_lines(old);
    let new_lines = split_diff_lines(new);
    let ops = diff_line_ops_with_whitespace(&old_lines, &new_lines, whitespace_mode);
    let mut insertions = 0;
    let mut deletions = 0;
    for (start, end) in unified_hunk_ranges(&ops, 3, 0) {
        if hunk_ignored_by_matching_lines(&ops, start, end, ignore_matching_lines) {
            continue;
        }
        for op in &ops[start..end] {
            match op {
                DiffLineOp::Equal(_) => {}
                DiffLineOp::Delete(_) => deletions += 1,
                DiffLineOp::Insert(_) => insertions += 1,
            }
        }
    }
    (insertions, deletions)
}

pub(crate) fn print_diff_stat_summary(rows: &[DiffStatRow]) {
    let files = rows.len();
    let insertions = rows.iter().map(|row| row.insertions).sum::<usize>();
    let deletions = rows.iter().map(|row| row.deletions).sum::<usize>();
    let mut summary = format!(" {} {} changed", files, plural(files, "file", "files"));
    if insertions > 0 || (insertions == 0 && deletions == 0) {
        summary.push_str(&format!(
            ", {} {}",
            insertions,
            plural(insertions, "insertion(+)", "insertions(+)")
        ));
    }
    if deletions > 0 || (insertions == 0 && deletions == 0) {
        summary.push_str(&format!(
            ", {} {}",
            deletions,
            plural(deletions, "deletion(-)", "deletions(-)")
        ));
    }
    println!("{summary}");
}

pub(crate) fn plural<'a>(value: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if value == 1 { singular } else { plural }
}

pub(crate) fn print_patch_entries(
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[skron_git_core::IndexDiffEntry],
    format: PatchFormatOptions,
) -> Result<()> {
    let mut out = io::stdout().lock();
    write_patch_entries(&mut out, repo, store, old_index, new_index, entries, format)
}

#[derive(Clone)]
pub(crate) struct PatchFormatOptions {
    pub(crate) old_source: DiffSideSource,
    pub(crate) new_source: DiffSideSource,
    pub(crate) word_diff: WordDiffMode,
    pub(crate) abbrev_len: Option<usize>,
    pub(crate) old_prefix: String,
    pub(crate) new_prefix: String,
    pub(crate) unified_context: usize,
    pub(crate) inter_hunk_context: usize,
    pub(crate) output_indicator_new: Option<u8>,
    pub(crate) output_indicator_old: Option<u8>,
    pub(crate) output_indicator_context: Option<u8>,
    pub(crate) ignore_matching_lines: Vec<Regex>,
    pub(crate) whitespace_mode: DiffWhitespaceMode,
    pub(crate) relative_prefix: Option<Vec<u8>>,
    pub(crate) text: bool,
    pub(crate) binary: bool,
    pub(crate) irreversible_delete: bool,
    pub(crate) submodule_format: SubmoduleDiffFormat,
    pub(crate) color_mode: DiffColorMode,
    pub(crate) emit_hunk_headers: bool,
}

impl PatchFormatOptions {
    pub(crate) fn cached() -> Self {
        Self {
            old_source: DiffSideSource::Index,
            new_source: DiffSideSource::Index,
            word_diff: WordDiffMode::None,
            abbrev_len: None,
            old_prefix: "a/".to_owned(),
            new_prefix: "b/".to_owned(),
            unified_context: 3,
            inter_hunk_context: 0,
            output_indicator_new: Some(b'+'),
            output_indicator_old: Some(b'-'),
            output_indicator_context: Some(b' '),
            ignore_matching_lines: Vec::new(),
            whitespace_mode: DiffWhitespaceMode::None,
            relative_prefix: None,
            text: false,
            binary: false,
            irreversible_delete: false,
            submodule_format: SubmoduleDiffFormat::Short,
            color_mode: DiffColorMode::Never,
            emit_hunk_headers: true,
        }
    }

    pub(crate) fn worktree() -> Self {
        Self {
            old_source: DiffSideSource::Index,
            new_source: DiffSideSource::WorktreeOrIndex,
            word_diff: WordDiffMode::None,
            abbrev_len: None,
            old_prefix: "a/".to_owned(),
            new_prefix: "b/".to_owned(),
            unified_context: 3,
            inter_hunk_context: 0,
            output_indicator_new: Some(b'+'),
            output_indicator_old: Some(b'-'),
            output_indicator_context: Some(b' '),
            ignore_matching_lines: Vec::new(),
            whitespace_mode: DiffWhitespaceMode::None,
            relative_prefix: None,
            text: false,
            binary: false,
            irreversible_delete: false,
            submodule_format: SubmoduleDiffFormat::Short,
            color_mode: DiffColorMode::Never,
            emit_hunk_headers: true,
        }
    }

    pub(crate) fn with_abbrev_len(mut self, abbrev_len: Option<usize>) -> Self {
        self.abbrev_len = abbrev_len;
        self
    }

    pub(crate) fn with_prefixes(mut self, old_prefix: String, new_prefix: String) -> Self {
        self.old_prefix = old_prefix;
        self.new_prefix = new_prefix;
        self
    }

    pub(crate) fn with_context(
        mut self,
        unified_context: usize,
        inter_hunk_context: usize,
    ) -> Self {
        self.unified_context = unified_context;
        self.inter_hunk_context = inter_hunk_context;
        self
    }

    pub(crate) fn with_whitespace_mode(mut self, whitespace_mode: DiffWhitespaceMode) -> Self {
        self.whitespace_mode = whitespace_mode;
        self
    }

    pub(crate) fn with_ignore_matching_lines(mut self, ignore_matching_lines: Vec<Regex>) -> Self {
        self.ignore_matching_lines = ignore_matching_lines;
        self
    }

    pub(crate) fn with_binary(mut self, binary: bool) -> Self {
        self.binary = binary;
        self
    }

    pub(crate) fn with_irreversible_delete(mut self, irreversible_delete: bool) -> Self {
        self.irreversible_delete = irreversible_delete;
        self
    }

    pub(crate) fn with_submodule_format(mut self, submodule_format: SubmoduleDiffFormat) -> Self {
        self.submodule_format = submodule_format;
        self
    }

    pub(crate) fn with_color_mode(mut self, color_mode: DiffColorMode) -> Self {
        self.color_mode = color_mode;
        self
    }

    pub(crate) fn with_hunk_headers(mut self, emit_hunk_headers: bool) -> Self {
        self.emit_hunk_headers = emit_hunk_headers;
        self
    }
}

pub(crate) fn write_patch_entries<W: Write>(
    out: &mut W,
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[skron_git_core::IndexDiffEntry],
    format: PatchFormatOptions,
) -> Result<()> {
    let abbrev_len = format.abbrev_len.unwrap_or(default_abbrev_len(store)?);
    let context = PatchWriteContext {
        repo,
        store,
        old_source: format.old_source,
        new_source: format.new_source,
        abbrev_len,
        old_prefix: &format.old_prefix,
        new_prefix: &format.new_prefix,
        unified_context: format.unified_context,
        inter_hunk_context: format.inter_hunk_context,
        output_indicator_new: format.output_indicator_new,
        output_indicator_old: format.output_indicator_old,
        output_indicator_context: format.output_indicator_context,
        ignore_matching_lines: &format.ignore_matching_lines,
        whitespace_mode: format.whitespace_mode,
        relative_prefix: format.relative_prefix.as_deref(),
        text: format.text,
        binary: format.binary,
        irreversible_delete: format.irreversible_delete,
        submodule_format: format.submodule_format,
        color: format.color_mode.enabled(),
        emit_hunk_headers: format.emit_hunk_headers,
    };
    for entry in entries {
        let old_entry = find_index_entry(old_index, diff_entry_old_path(entry));
        let new_entry = find_index_entry(new_index, &entry.path);
        write_patch_entry(
            out,
            &context,
            entry,
            old_entry,
            new_entry,
            format.word_diff,
            None,
        )?;
    }
    Ok(())
}

fn write_patch_entries_streaming_from_tree_diff<W: Write, S: GitObjectStore + ?Sized>(
    out: &mut W,
    tree_cache: &TreeObjectCache<'_, S>,
    old_tree: Option<&ObjectId>,
    new_tree: &ObjectId,
    context: PatchWriteContext<'_>,
    word_diff: WordDiffMode,
    blob_cache: &mut FormatPatchBlobCache<'_>,
) -> Result<()> {
    for entry in skron_git_core::diff_trees(tree_cache, old_tree, new_tree)? {
        let _trace = phase_trace("format_patch.write_tree_diff.entry");
        let old_entry = entry
            .old_entry
            .as_ref()
            .map(index_entry_from_tree_diff_file);
        let new_entry = entry
            .new_entry
            .as_ref()
            .map(index_entry_from_tree_diff_file);
        write_patch_entry_view(
            out,
            &context,
            PatchEntryView {
                status: entry.status,
                path: &entry.path,
                old_path: None,
                similarity: None,
                old_entry: old_entry.as_ref(),
                new_entry: new_entry.as_ref(),
            },
            word_diff,
            Some(blob_cache),
        )?;
    }
    Ok(())
}

fn index_entry_from_tree_diff_file(entry: &skron_git_core::TreeDiffFileEntry) -> IndexEntry {
    IndexEntry::new(entry.path.to_vec(), entry.id.clone(), entry.mode, 0)
        .expect("tree diff entries have valid git index paths")
}

struct PatchEntryView<'a> {
    status: IndexDiffStatus,
    path: &'a [u8],
    old_path: Option<&'a [u8]>,
    similarity: Option<u8>,
    old_entry: Option<&'a IndexEntry>,
    new_entry: Option<&'a IndexEntry>,
}

fn patch_entry_old_path<'a>(view: &'a PatchEntryView<'a>) -> &'a [u8] {
    view.old_path.unwrap_or(view.path)
}

fn patch_entry_has_binary_prefix(
    entry: &PatchEntryView<'_>,
    old_entry: Option<&IndexEntry>,
    new_entry: Option<&IndexEntry>,
    cache: &mut FormatPatchBlobCache<'_>,
) -> Result<bool> {
    if !matches!(
        entry.status,
        IndexDiffStatus::Added | IndexDiffStatus::Deleted | IndexDiffStatus::Modified
    ) {
        return Ok(false);
    }
    if let Some(old_entry) = old_entry
        && cache.index_blob_binary_prefix(old_entry)?
    {
        return Ok(true);
    }
    if let Some(new_entry) = new_entry
        && cache.index_blob_binary_prefix(new_entry)?
    {
        return Ok(true);
    }
    Ok(false)
}

struct PatchBinarySummary<'a> {
    entry: &'a PatchEntryView<'a>,
    old_entry: Option<&'a IndexEntry>,
    new_entry: Option<&'a IndexEntry>,
    old_display_path: &'a str,
    new_display_path: &'a str,
    mode: &'a str,
}

fn write_patch_entry_binary_summary<W: Write>(
    out: &mut W,
    context: &PatchWriteContext<'_>,
    summary: PatchBinarySummary<'_>,
) -> Result<()> {
    write_patch_meta_line(
        out,
        context.color,
        format_args!(
            "diff --git {old_prefix}{old_display_path} {new_prefix}{new_display_path}",
            old_display_path = summary.old_display_path,
            new_display_path = summary.new_display_path,
            old_prefix = context.old_prefix,
            new_prefix = context.new_prefix
        ),
    )?;

    match summary.entry.status {
        IndexDiffStatus::Added => writeln!(out, "new file mode {}", summary.mode)?,
        IndexDiffStatus::Deleted => writeln!(out, "deleted file mode {}", summary.mode)?,
        IndexDiffStatus::Modified => {}
        IndexDiffStatus::Copied | IndexDiffStatus::Renamed => unreachable!(),
    }
    if matches!(summary.entry.status, IndexDiffStatus::Modified) {
        if let Some(dissimilarity) = summary.entry.similarity {
            writeln!(out, "dissimilarity index {dissimilarity}%")?;
        }
        write_patch_index_line(
            out,
            context.color,
            summary.old_entry.map(|entry| &entry.id),
            summary.new_entry.map(|entry| &entry.id),
            context.abbrev_len,
            context.binary,
            Some(summary.mode),
        )?;
    } else {
        write_patch_index_line(
            out,
            context.color,
            summary.old_entry.map(|entry| &entry.id),
            summary.new_entry.map(|entry| &entry.id),
            context.abbrev_len,
            context.binary,
            None,
        )?;
    }
    write_binary_files_line(
        out,
        if summary.entry.status == IndexDiffStatus::Added {
            None
        } else {
            Some((context.old_prefix, summary.old_display_path))
        },
        if summary.entry.status == IndexDiffStatus::Deleted {
            None
        } else {
            Some((context.new_prefix, summary.new_display_path))
        },
    )
}

pub(crate) struct PatchWriteContext<'a> {
    pub(crate) repo: &'a GitRepo,
    pub(crate) store: &'a LooseObjectStore,
    pub(crate) old_source: DiffSideSource,
    pub(crate) new_source: DiffSideSource,
    pub(crate) abbrev_len: usize,
    pub(crate) old_prefix: &'a str,
    pub(crate) new_prefix: &'a str,
    pub(crate) unified_context: usize,
    pub(crate) inter_hunk_context: usize,
    pub(crate) output_indicator_new: Option<u8>,
    pub(crate) output_indicator_old: Option<u8>,
    pub(crate) output_indicator_context: Option<u8>,
    pub(crate) ignore_matching_lines: &'a [Regex],
    pub(crate) whitespace_mode: DiffWhitespaceMode,
    pub(crate) relative_prefix: Option<&'a [u8]>,
    pub(crate) text: bool,
    pub(crate) binary: bool,
    pub(crate) irreversible_delete: bool,
    pub(crate) submodule_format: SubmoduleDiffFormat,
    pub(crate) color: bool,
    pub(crate) emit_hunk_headers: bool,
}

pub(crate) fn write_patch_entry<W: Write>(
    out: &mut W,
    context: &PatchWriteContext<'_>,
    entry: &skron_git_core::IndexDiffEntry,
    old_entry: Option<&IndexEntry>,
    new_entry: Option<&IndexEntry>,
    word_diff: WordDiffMode,
    mut blob_cache: Option<&mut FormatPatchBlobCache<'_>>,
) -> Result<()> {
    write_patch_entry_view(
        out,
        context,
        PatchEntryView {
            status: entry.status,
            path: &entry.path,
            old_path: entry.old_path.as_deref(),
            similarity: entry.similarity,
            old_entry,
            new_entry,
        },
        word_diff,
        blob_cache.take(),
    )
}

fn write_patch_entry_view<W: Write>(
    out: &mut W,
    context: &PatchWriteContext<'_>,
    entry: PatchEntryView<'_>,
    word_diff: WordDiffMode,
    mut blob_cache: Option<&mut FormatPatchBlobCache<'_>>,
) -> Result<()> {
    let old_entry = entry.old_entry;
    let new_entry = entry.new_entry;
    let (old_display_path, new_display_path) = {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_display_paths");
        (
            diff_display_path(patch_entry_old_path(&entry), context.relative_prefix),
            diff_display_path(entry.path, context.relative_prefix),
        )
    };
    let new_mode = new_entry.map(|entry| index_mode_octal(entry.mode));
    let old_mode = old_entry.map(|entry| index_mode_octal(entry.mode));
    let mode = new_mode.or(old_mode).unwrap_or("100644");
    if (old_entry.is_some_and(|entry| entry.mode == IndexMode::Gitlink)
        || new_entry.is_some_and(|entry| entry.mode == IndexMode::Gitlink))
        && context.submodule_format != SubmoduleDiffFormat::Short
    {
        let submodule_entry = skron_git_core::IndexDiffEntry {
            status: entry.status,
            path: entry.path.to_vec(),
            old_path: entry.old_path.map(|path| path.to_vec()),
            similarity: entry.similarity,
        };
        return write_submodule_diff_entry(
            out,
            context,
            &submodule_entry,
            old_entry,
            new_entry,
            &new_display_path,
        );
    }

    match entry.status {
        IndexDiffStatus::Copied => {
            write_patch_meta_line(
                out,
                context.color,
                format_args!(
                    "diff --git {old_prefix}{old_display_path} {new_prefix}{new_display_path}",
                    old_display_path = old_display_path,
                    old_prefix = context.old_prefix,
                    new_prefix = context.new_prefix
                ),
            )?;
            writeln!(out, "similarity index {}%", entry.similarity.unwrap_or(100))?;
            writeln!(out, "copy from {old_display_path}")?;
            writeln!(out, "copy to {new_display_path}")?;
            return Ok(());
        }
        IndexDiffStatus::Renamed => {
            write_patch_meta_line(
                out,
                context.color,
                format_args!(
                    "diff --git {old_prefix}{old_display_path} {new_prefix}{new_display_path}",
                    old_display_path = old_display_path,
                    old_prefix = context.old_prefix,
                    new_prefix = context.new_prefix
                ),
            )?;
            writeln!(out, "similarity index {}%", entry.similarity.unwrap_or(100))?;
            writeln!(out, "rename from {old_display_path}")?;
            writeln!(out, "rename to {new_display_path}")?;
            return Ok(());
        }
        _ => {}
    }

    enum CachedContent<'a> {
        Owned(Vec<u8>),
        Borrowed(&'a [u8]),
    }
    impl CachedContent<'_> {
        fn as_slice(&self) -> &[u8] {
            match self {
                Self::Owned(content) => content,
                Self::Borrowed(content) => content,
            }
        }
    }

    if matches!(entry.status, IndexDiffStatus::Modified)
        && let (Some(old_entry), Some(new_entry)) = (old_entry, new_entry)
        && old_entry.id == new_entry.id
        && old_entry.mode != new_entry.mode
    {
        let old_mode = index_mode_octal(old_entry.mode);
        let new_mode = index_mode_octal(new_entry.mode);
        write_patch_meta_line(
            out,
            context.color,
            format_args!(
                "diff --git {old_prefix}{old_display_path} {new_prefix}{new_display_path}",
                old_display_path = old_display_path,
                old_prefix = context.old_prefix,
                new_prefix = context.new_prefix
            ),
        )?;
        writeln!(out, "old mode {old_mode}")?;
        writeln!(out, "new mode {new_mode}")?;
        return Ok(());
    }

    if !context.text
        && !context.binary
        && context.old_source == DiffSideSource::Index
        && context.new_source == DiffSideSource::Index
        && let Some(cache) = blob_cache.as_deref_mut()
        && {
            let _trace = phase_trace("format_patch.write_tree_diff.entry_binary_prefix");
            patch_entry_has_binary_prefix(&entry, old_entry, new_entry, cache)?
        }
    {
        write_patch_entry_binary_summary(
            out,
            context,
            PatchBinarySummary {
                entry: &entry,
                old_entry,
                new_entry,
                old_display_path: &old_display_path,
                new_display_path: &new_display_path,
                mode,
            },
        )?;
        return Ok(());
    }

    let (old_content, new_content) = if matches!(
        entry.status,
        IndexDiffStatus::Added | IndexDiffStatus::Deleted | IndexDiffStatus::Modified
    ) {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_content");
        if context.old_source == DiffSideSource::Index
            && context.new_source == DiffSideSource::Index
            && let Some(cache) = blob_cache.as_deref_mut()
        {
            {
                let _trace = phase_trace("format_patch.write_tree_diff.entry_ensure_blobs");
                if let Some(entry) = old_entry {
                    cache.ensure_index_blob(entry)?;
                }
                if let Some(entry) = new_entry {
                    cache.ensure_index_blob(entry)?;
                }
            }
            (
                old_entry
                    .map(|entry| {
                        let _trace =
                            phase_trace("format_patch.write_tree_diff.entry_content.cached_old");
                        CachedContent::Borrowed(cache.index_blob_or_empty(entry))
                    })
                    .unwrap_or(CachedContent::Borrowed(&[])),
                new_entry
                    .map(|new_index_entry| {
                        let _trace =
                            phase_trace("format_patch.write_tree_diff.entry_content.cached_new");
                        let _status_trace = phase_trace(match entry.status {
                            IndexDiffStatus::Added => {
                                "format_patch.write_tree_diff.entry_content.cached_new.added"
                            }
                            IndexDiffStatus::Deleted => {
                                "format_patch.write_tree_diff.entry_content.cached_new.deleted"
                            }
                            IndexDiffStatus::Modified => {
                                "format_patch.write_tree_diff.entry_content.cached_new.modified"
                            }
                            IndexDiffStatus::Copied | IndexDiffStatus::Renamed => {
                                "format_patch.write_tree_diff.entry_content.cached_new.other"
                            }
                        });
                        let content = cache.index_blob_or_empty(new_index_entry);
                        let _size_trace = phase_trace(match entry.status {
                            IndexDiffStatus::Added if content.len() <= 256 => {
                                match count_diff_lines(content) {
                                    0 | 1 => {
                                        "format_patch.write_tree_diff.entry_content.cached_new.added.small.tiny.single_line"
                                    }
                                    2 => {
                                        "format_patch.write_tree_diff.entry_content.cached_new.added.small.tiny.multi_line.two"
                                    }
                                    3 | 4 => {
                                        "format_patch.write_tree_diff.entry_content.cached_new.added.small.tiny.multi_line.three_four"
                                    }
                                    _ => {
                                        "format_patch.write_tree_diff.entry_content.cached_new.added.small.tiny.multi_line.five_plus"
                                    }
                                }
                            }
                            IndexDiffStatus::Added if content.len() <= 1024 => {
                                "format_patch.write_tree_diff.entry_content.cached_new.added.small.mid"
                            }
                            IndexDiffStatus::Added if content.len() <= BINARY_DETECTION_BYTES => {
                                "format_patch.write_tree_diff.entry_content.cached_new.added.small.upper"
                            }
                            IndexDiffStatus::Added => {
                                "format_patch.write_tree_diff.entry_content.cached_new.added.large"
                            }
                            _ => "format_patch.write_tree_diff.entry_content.cached_new.other_size",
                        });
                        let _ = &_size_trace;
                        CachedContent::Borrowed(content)
                    })
                    .unwrap_or(CachedContent::Borrowed(&[])),
            )
        } else {
            (
                old_entry
                    .map(|entry| {
                        read_diff_side_content(
                            context.repo,
                            context.store,
                            entry,
                            context.old_source,
                        )
                        .map(CachedContent::Owned)
                    })
                    .transpose()?
                    .unwrap_or(CachedContent::Borrowed(&[])),
                new_entry
                    .map(|entry| {
                        read_diff_side_content(
                            context.repo,
                            context.store,
                            entry,
                            context.new_source,
                        )
                        .map(CachedContent::Owned)
                    })
                    .transpose()?
                    .unwrap_or(CachedContent::Borrowed(&[])),
            )
        }
    } else {
        (CachedContent::Borrowed(&[]), CachedContent::Borrowed(&[]))
    };
    let old_content = old_content.as_slice();
    let new_content = new_content.as_slice();
    let binary = {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_binary_detect");
        if context.text {
            false
        } else {
            let _trace =
                phase_trace("format_patch.write_tree_diff.entry_binary_detect.content_scan");
            let _status_trace = phase_trace(match entry.status {
                IndexDiffStatus::Added => {
                    "format_patch.write_tree_diff.entry_binary_detect.content_scan.added"
                }
                IndexDiffStatus::Deleted => {
                    "format_patch.write_tree_diff.entry_binary_detect.content_scan.deleted"
                }
                IndexDiffStatus::Modified => {
                    "format_patch.write_tree_diff.entry_binary_detect.content_scan.modified"
                }
                IndexDiffStatus::Copied | IndexDiffStatus::Renamed => {
                    "format_patch.write_tree_diff.entry_binary_detect.content_scan.other"
                }
            });
            let _size_trace = phase_trace(match entry.status {
                IndexDiffStatus::Added if new_content.len() <= 256 => {
                    match count_diff_lines(new_content) {
                        0 | 1 => {
                            "format_patch.write_tree_diff.entry_binary_detect.content_scan.added.small.tiny.single_line"
                        }
                        2 => {
                            "format_patch.write_tree_diff.entry_binary_detect.content_scan.added.small.tiny.multi_line.two"
                        }
                        3 | 4 => {
                            "format_patch.write_tree_diff.entry_binary_detect.content_scan.added.small.tiny.multi_line.three_four"
                        }
                        _ => {
                            "format_patch.write_tree_diff.entry_binary_detect.content_scan.added.small.tiny.multi_line.five_plus"
                        }
                    }
                }
                IndexDiffStatus::Added if new_content.len() <= 1024 => {
                    "format_patch.write_tree_diff.entry_binary_detect.content_scan.added.small.mid"
                }
                IndexDiffStatus::Added if new_content.len() <= BINARY_DETECTION_BYTES => {
                    "format_patch.write_tree_diff.entry_binary_detect.content_scan.added.small.upper"
                }
                IndexDiffStatus::Added => {
                    "format_patch.write_tree_diff.entry_binary_detect.content_scan.added.large"
                }
                _ => "format_patch.write_tree_diff.entry_binary_detect.content_scan.other_size",
            });
            let _ = &_size_trace;
            is_binary_content(old_content) || is_binary_content(new_content)
        }
    };
    if entry.status == IndexDiffStatus::Modified
        && !binary
        && (context.whitespace_mode != DiffWhitespaceMode::None
            || !context.ignore_matching_lines.is_empty())
    {
        let visible_changes = {
            let _trace = phase_trace("format_patch.write_tree_diff.entry_visible_changes");
            diff_line_counts_with_options(
                old_content,
                new_content,
                context.whitespace_mode,
                &context.ignore_matching_lines,
            )
        };
        if visible_changes == (0, 0) {
            return Ok(());
        }
    }

    {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_meta");
        write_patch_meta_line(
            out,
            context.color,
            format_args!(
                "diff --git {old_prefix}{old_display_path} {new_prefix}{new_display_path}",
                old_display_path = old_display_path,
                old_prefix = context.old_prefix,
                new_prefix = context.new_prefix
            ),
        )?;

        match entry.status {
            IndexDiffStatus::Added => writeln!(out, "new file mode {mode}")?,
            IndexDiffStatus::Deleted => writeln!(out, "deleted file mode {mode}")?,
            IndexDiffStatus::Modified => {}
            IndexDiffStatus::Copied | IndexDiffStatus::Renamed => unreachable!(),
        }
        if matches!(entry.status, IndexDiffStatus::Modified) {
            if let Some(dissimilarity) = entry.similarity {
                writeln!(out, "dissimilarity index {dissimilarity}%")?;
            }
            write_patch_index_line(
                out,
                context.color,
                old_entry.map(|entry| &entry.id),
                new_entry.map(|entry| &entry.id),
                context.abbrev_len,
                context.binary,
                Some(mode),
            )?;
        } else {
            write_patch_index_line(
                out,
                context.color,
                old_entry.map(|entry| &entry.id),
                new_entry.map(|entry| &entry.id),
                context.abbrev_len,
                context.binary,
                None,
            )?;
        }
    }

    if old_entry.is_some_and(|entry| entry.mode == IndexMode::Gitlink)
        || new_entry.is_some_and(|entry| entry.mode == IndexMode::Gitlink)
    {
        if context.irreversible_delete && entry.status == IndexDiffStatus::Deleted {
            return Ok(());
        }
        write_path_line(
            out,
            context.color,
            b"---",
            if entry.status == IndexDiffStatus::Added {
                None
            } else {
                Some((context.old_prefix, old_display_path.as_ref()))
            },
        )?;
        write_path_line(
            out,
            context.color,
            b"+++",
            if entry.status == IndexDiffStatus::Deleted {
                None
            } else {
                Some((context.new_prefix, new_display_path.as_ref()))
            },
        )?;
        let old_gitlink = old_entry
            .map(|entry| format!("Subproject commit {}\n", entry.id.to_hex()))
            .unwrap_or_default();
        let new_gitlink = new_entry
            .map(|entry| format!("Subproject commit {}\n", entry.id.to_hex()))
            .unwrap_or_default();
        return write_unified_full_file_hunk(
            out,
            old_gitlink.as_bytes(),
            new_gitlink.as_bytes(),
            &new_display_path,
            HunkFormatOptions {
                word_diff,
                color: context.color,
                unified_context: context.unified_context,
                inter_hunk_context: context.inter_hunk_context,
                output_indicator_new: context.output_indicator_new,
                output_indicator_old: context.output_indicator_old,
                output_indicator_context: context.output_indicator_context,
                ignore_matching_lines: context.ignore_matching_lines,
                whitespace_mode: context.whitespace_mode,
                emit_hunk_headers: context.emit_hunk_headers,
            },
        );
    }
    if old_content.is_empty() && new_content.is_empty() {
        return Ok(());
    }
    if binary {
        if context.binary {
            write_git_binary_patch(out, new_content, old_content)?;
            return Ok(());
        }
        write_binary_files_line(
            out,
            if entry.status == IndexDiffStatus::Added {
                None
            } else {
                Some((context.old_prefix, old_display_path.as_ref()))
            },
            if entry.status == IndexDiffStatus::Deleted {
                None
            } else {
                Some((context.new_prefix, new_display_path.as_ref()))
            },
        )?;
        return Ok(());
    }
    if context.irreversible_delete && entry.status == IndexDiffStatus::Deleted {
        return Ok(());
    }

    {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk");
        write_path_line(
            out,
            context.color,
            b"---",
            if entry.status == IndexDiffStatus::Added {
                None
            } else {
                Some((context.old_prefix, old_display_path.as_ref()))
            },
        )?;
        write_path_line(
            out,
            context.color,
            b"+++",
            if entry.status == IndexDiffStatus::Deleted {
                None
            } else {
                Some((context.new_prefix, new_display_path.as_ref()))
            },
        )?;
        write_unified_full_file_hunk(
            out,
            old_content,
            new_content,
            &new_display_path,
            HunkFormatOptions {
                word_diff,
                color: context.color,
                unified_context: context.unified_context,
                inter_hunk_context: context.inter_hunk_context,
                output_indicator_new: context.output_indicator_new,
                output_indicator_old: context.output_indicator_old,
                output_indicator_context: context.output_indicator_context,
                ignore_matching_lines: context.ignore_matching_lines,
                whitespace_mode: context.whitespace_mode,
                emit_hunk_headers: context.emit_hunk_headers,
            },
        )
    }
}

pub(crate) fn write_submodule_diff_entry<W: Write>(
    out: &mut W,
    context: &PatchWriteContext<'_>,
    entry: &skron_git_core::IndexDiffEntry,
    old_entry: Option<&IndexEntry>,
    new_entry: Option<&IndexEntry>,
    display_path: &str,
) -> Result<()> {
    if context.irreversible_delete && entry.status == IndexDiffStatus::Deleted {
        return Ok(());
    }
    let path = context
        .repo
        .root
        .join(String::from_utf8_lossy(&entry.path).as_ref());
    let submodule_repo = exact_repo_at(&path)
        .ok_or_else(|| CliError::Message(format!("not a git repository: {}", path.display())))?;
    let submodule_store =
        LooseObjectStore::new(submodule_repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let old_id = old_entry
        .map(|entry| entry.id.clone())
        .unwrap_or_else(zero_object_id);
    let new_id = new_entry
        .map(|entry| entry.id.clone())
        .unwrap_or_else(zero_object_id);
    writeln!(
        out,
        "Submodule {display_path} {}..{}:",
        short_object_id(&old_id),
        short_object_id(&new_id)
    )?;
    match context.submodule_format {
        SubmoduleDiffFormat::Short => Ok(()),
        SubmoduleDiffFormat::Log => {
            let commit_cache = CommitObjectCache::new(&submodule_store);
            for id in submodule_commit_range(&submodule_repo, &submodule_store, &old_id, &new_id)? {
                let commit = commit_cache.read_commit(&id)?;
                writeln!(out, "  > {}", commit_subject(&commit.message))?;
            }
            Ok(())
        }
        SubmoduleDiffFormat::Diff => {
            let old_index = if old_entry.is_some() {
                read_treeish_index(&submodule_repo, &submodule_store, &old_id.to_hex())?
            } else {
                GitIndex::new()
            };
            let new_index = if new_entry.is_some() {
                read_treeish_index(&submodule_repo, &submodule_store, &new_id.to_hex())?
            } else {
                GitIndex::new()
            };
            let entries = diff_indexes(&old_index, &new_index)?;
            write_patch_entries(
                out,
                &submodule_repo,
                &submodule_store,
                &old_index,
                &new_index,
                &entries,
                PatchFormatOptions::cached()
                    .with_prefixes(
                        format!("{}{display_path}/", context.old_prefix),
                        format!("{}{display_path}/", context.new_prefix),
                    )
                    .with_context(context.unified_context, context.inter_hunk_context)
                    .with_submodule_format(SubmoduleDiffFormat::Short)
                    .with_color_mode(if context.color {
                        DiffColorMode::Always
                    } else {
                        DiffColorMode::Never
                    }),
            )
        }
    }
}

pub(crate) fn submodule_commit_range(
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_id: &ObjectId,
    new_id: &ObjectId,
) -> Result<Vec<ObjectId>> {
    if *new_id == zero_object_id() {
        return Ok(Vec::new());
    }
    let rev = if *old_id == zero_object_id() {
        new_id.to_hex()
    } else {
        format!("{}..{}", old_id.to_hex(), new_id.to_hex())
    };
    let revs = collect_rev_list_revs(repo, store, false, vec![rev])?;
    let mut commits = collect_commits_with_exclusions(repo, store, &revs, None)?;
    commits.reverse();
    Ok(commits)
}

pub(crate) fn write_git_binary_patch<W: Write>(
    out: &mut W,
    forward: &[u8],
    reverse: &[u8],
) -> Result<()> {
    writeln!(out, "GIT binary patch")?;
    write_git_binary_literal(out, forward)?;
    writeln!(out)?;
    write_git_binary_literal(out, reverse)?;
    writeln!(out)?;
    Ok(())
}

pub(crate) fn write_git_binary_literal<W: Write>(out: &mut W, content: &[u8]) -> Result<()> {
    writeln!(out, "literal {}", content.len())?;
    let compressed = zlib_compress(content)?;
    for chunk in compressed.chunks(52) {
        out.write_all(&[git_base85_length_char(chunk.len())])?;
        out.write_all(&git_base85_encode(chunk))?;
        writeln!(out)?;
    }
    Ok(())
}

pub(crate) fn zlib_compress(content: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(content)?;
    encoder.finish().map_err(CliError::Io)
}

pub(crate) fn git_base85_length_char(len: usize) -> u8 {
    debug_assert!((1..=52).contains(&len));
    if len <= 26 {
        b'A' + (len as u8) - 1
    } else {
        b'a' + (len as u8) - 27
    }
}

pub(crate) fn git_base85_encode(bytes: &[u8]) -> Vec<u8> {
    const ALPHABET: &[u8; 85] =
        b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";
    let mut encoded = Vec::with_capacity(bytes.len().div_ceil(4) * 5);
    for chunk in bytes.chunks(4) {
        let mut word_bytes = [0_u8; 4];
        word_bytes[..chunk.len()].copy_from_slice(chunk);
        let mut value = u32::from_be_bytes(word_bytes);
        let mut digits = [0_u8; 5];
        for digit in digits.iter_mut().rev() {
            *digit = ALPHABET[(value % 85) as usize];
            value /= 85;
        }
        encoded.extend_from_slice(&digits);
    }
    encoded
}

pub(crate) fn read_index_entry_content(
    store: &LooseObjectStore,
    entry: &IndexEntry,
) -> Result<Vec<u8>> {
    if entry.mode == IndexMode::Gitlink {
        return Ok(Vec::new());
    }
    let object = {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_content.index.read_object");
        store.read_object(&entry.id)?
    };
    if object.kind != GitObjectKind::Blob {
        return Err(CliError::Fatal {
            code: 128,
            message: "diff index entry does not point to a blob".into(),
        });
    }
    Ok(object.content)
}

pub(crate) fn read_worktree_or_index_entry_content(
    repo: &GitRepo,
    store: &LooseObjectStore,
    entry: &IndexEntry,
) -> Result<Vec<u8>> {
    if entry.mode == IndexMode::Gitlink {
        return Ok(Vec::new());
    }
    let path = repo
        .root
        .join(String::from_utf8_lossy(&entry.path).as_ref());
    match {
        let _trace = phase_trace(
            "format_patch.write_tree_diff.entry_content.worktree_or_index.read_worktree",
        );
        fs::read(path)
    } {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let _trace = phase_trace(
                "format_patch.write_tree_diff.entry_content.worktree_or_index.fallback_index",
            );
            read_index_entry_content(store, entry)
        }
        Err(error) => Err(CliError::Io(error)),
    }
}

const BINARY_DETECTION_BYTES: usize = 8000;

pub(crate) fn is_binary_content(content: &[u8]) -> bool {
    content[..content.len().min(BINARY_DETECTION_BYTES)].contains(&0)
}

pub(crate) fn print_unified_full_file_hunk(old: &[u8], new: &[u8], path: &str) -> Result<()> {
    let mut out = io::stdout().lock();
    write_unified_full_file_hunk(&mut out, old, new, path, HunkFormatOptions::default())
}

#[derive(Clone, Copy)]
pub(crate) struct HunkFormatOptions<'a> {
    word_diff: WordDiffMode,
    color: bool,
    unified_context: usize,
    inter_hunk_context: usize,
    output_indicator_new: Option<u8>,
    output_indicator_old: Option<u8>,
    output_indicator_context: Option<u8>,
    ignore_matching_lines: &'a [Regex],
    whitespace_mode: DiffWhitespaceMode,
    emit_hunk_headers: bool,
}

impl Default for HunkFormatOptions<'static> {
    fn default() -> Self {
        Self {
            word_diff: WordDiffMode::None,
            color: false,
            unified_context: 3,
            inter_hunk_context: 0,
            output_indicator_new: Some(b'+'),
            output_indicator_old: Some(b'-'),
            output_indicator_context: Some(b' '),
            ignore_matching_lines: &[],
            whitespace_mode: DiffWhitespaceMode::None,
            emit_hunk_headers: true,
        }
    }
}

pub(crate) fn write_unified_full_file_hunk<W: Write>(
    out: &mut W,
    old: &[u8],
    new: &[u8],
    path: &str,
    format: HunkFormatOptions<'_>,
) -> Result<()> {
    if format.word_diff == WordDiffMode::None
        && format.whitespace_mode == DiffWhitespaceMode::None
        && format.ignore_matching_lines.is_empty()
        && (old.is_empty() || new.is_empty())
    {
        return write_unified_single_sided_hunk(out, old, new, path, format);
    }

    let (old_lines, new_lines) = {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_split_lines");
        (split_diff_lines(old), split_diff_lines(new))
    };
    let ops = {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_diff_ops");
        diff_line_ops_with_whitespace(&old_lines, &new_lines, format.whitespace_mode)
    };
    let header_cache = if format.emit_hunk_headers {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_header_cache");
        build_hunk_header_cache(&old_lines, path, format.unified_context)
    } else {
        None
    };

    let mut next_op_idx = 0usize;
    let mut old_line = 1usize;
    let mut new_line = 1usize;

    let hunk_ranges = {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_ranges");
        unified_hunk_ranges(&ops, format.unified_context, format.inter_hunk_context)
    };
    for (start, end) in hunk_ranges {
        while next_op_idx < start {
            match ops[next_op_idx] {
                DiffLineOp::Equal(_) => {
                    old_line += 1;
                    new_line += 1;
                }
                DiffLineOp::Delete(_) => old_line += 1,
                DiffLineOp::Insert(_) => new_line += 1,
            }
            next_op_idx += 1;
        }

        let mut old_count = 0usize;
        let mut new_count = 0usize;
        let hunk_old_start = old_line;
        let hunk_new_start = new_line;

        for op in &ops[start..end] {
            match op {
                DiffLineOp::Equal(_) => {
                    old_count += 1;
                    new_count += 1;
                    old_line += 1;
                    new_line += 1;
                }
                DiffLineOp::Delete(_) => {
                    old_count += 1;
                    old_line += 1;
                }
                DiffLineOp::Insert(_) => {
                    new_count += 1;
                    new_line += 1;
                }
            }
        }

        write_unified_hunk(
            out,
            &ops,
            &old_lines,
            path,
            start,
            end,
            &format,
            header_cache.as_ref(),
            hunk_old_start,
            hunk_new_start,
            old_count,
            new_count,
        )?;
        next_op_idx = end;
    }
    Ok(())
}

fn write_unified_single_sided_hunk<W: Write>(
    out: &mut W,
    old: &[u8],
    new: &[u8],
    path: &str,
    format: HunkFormatOptions<'_>,
) -> Result<()> {
    let (removed, added, removed_prefix, added_prefix) = if old.is_empty() {
        (&[][..], new, 0usize, 1usize)
    } else {
        (old, &[][..], 1usize, 0usize)
    };
    let (removed_lines, added_lines) = {
        let _trace =
            phase_trace("format_patch.write_tree_diff.entry_hunk_single_sided_split_lines");
        (split_diff_lines(removed), split_diff_lines(added))
    };
    let removed_count = removed_lines.len();
    let added_count = added_lines.len();

    {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_single_sided_header");
        if format.color {
            out.write_all(b"\x1b[36m")?;
        }
        out.write_all(b"@@ -")?;
        {
            let _trace =
                phase_trace("format_patch.write_tree_diff.entry_hunk_single_sided_header.range");
            write_unified_range(out, removed_prefix, removed_count)?;
        }
        out.write_all(b" +")?;
        {
            let _trace =
                phase_trace("format_patch.write_tree_diff.entry_hunk_single_sided_header.range");
            write_unified_range(out, added_prefix, added_count)?;
        }
        out.write_all(b" @@")?;
        if format.emit_hunk_headers && !removed_lines.is_empty() {
            let header = {
                let _trace = phase_trace(
                    "format_patch.write_tree_diff.entry_hunk_single_sided_header.context",
                );
                hunk_header(
                    &removed_lines,
                    path,
                    1,
                    removed_count,
                    format.unified_context,
                    None,
                )
            };
            if let Some(header) = header {
                write!(out, " {header}")?;
            }
        }
        if format.color {
            out.write_all(b"\x1b[m")?;
        }
        writeln!(out)?;
    }

    {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_single_sided_body");
        for line in removed_lines {
            write_diff_line(
                out,
                format.output_indicator_old,
                line,
                diff_line_color(format.color, DiffLineColor::Delete),
            )?;
        }
        for line in added_lines {
            write_diff_line(
                out,
                format.output_indicator_new,
                line,
                diff_line_color(format.color, DiffLineColor::Insert),
            )?;
        }
    }
    Ok(())
}

fn write_unified_hunk<W: Write>(
    out: &mut W,
    ops: &[DiffLineOp<'_>],
    old_lines: &[&[u8]],
    path: &str,
    start: usize,
    end: usize,
    format: &HunkFormatOptions<'_>,
    header_cache: Option<&HunkHeaderCache>,
    hunk_old_start: usize,
    hunk_new_start: usize,
    hunk_old_count: usize,
    hunk_new_count: usize,
) -> Result<()> {
    if hunk_ignored_by_matching_lines(ops, start, end, format.ignore_matching_lines) {
        return Ok(());
    }
    let old_start = if hunk_old_count == 0 {
        hunk_old_start.saturating_sub(1)
    } else {
        hunk_old_start
    };
    let new_start = if hunk_new_count == 0 {
        hunk_new_start.saturating_sub(1)
    } else {
        hunk_new_start
    };
    if format.color {
        out.write_all(b"\x1b[36m")?;
    }
    out.write_all(b"@@ -")?;
    write_unified_range(out, old_start, hunk_old_count)?;
    out.write_all(b" +")?;
    write_unified_range(out, new_start, hunk_new_count)?;
    out.write_all(b" @@")?;
    if format.emit_hunk_headers
        && let Some(header) = hunk_header(
            old_lines,
            path,
            old_start,
            hunk_old_count,
            format.unified_context,
            header_cache,
        )
    {
        write!(out, " {header}")?;
    }
    if format.color {
        out.write_all(b"\x1b[m")?;
    }
    writeln!(out)?;
    {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_body");
        let mut idx = start;
        while idx < end {
            let op = &ops[idx];
            match op {
                DiffLineOp::Equal(line) if format.word_diff == WordDiffMode::Plain => {
                    write_raw_diff_line(out, line)?
                }
                DiffLineOp::Equal(line) if format.word_diff == WordDiffMode::Color => {
                    write_colored_line_body(out, line)?
                }
                DiffLineOp::Equal(line) if format.word_diff == WordDiffMode::Porcelain => {
                    write_word_diff_porcelain_equal_line(out, line)?
                }
                DiffLineOp::Equal(line) => write_diff_line(
                    out,
                    format.output_indicator_context,
                    line,
                    diff_line_color(format.color, DiffLineColor::Context),
                )?,
                DiffLineOp::Delete(line) if format.word_diff != WordDiffMode::None => {
                    let next_idx =
                        write_word_diff_change_block(out, ops, idx, end, format.word_diff)?;
                    if next_idx != idx {
                        idx = next_idx;
                        continue;
                    }
                    write_word_diff_delete_line(out, line, format.word_diff)?
                }
                DiffLineOp::Delete(line) => write_diff_line(
                    out,
                    format.output_indicator_old,
                    line,
                    diff_line_color(format.color, DiffLineColor::Delete),
                )?,
                DiffLineOp::Insert(line) if format.word_diff != WordDiffMode::None => {
                    write_word_diff_insert_line(out, line, format.word_diff)?
                }
                DiffLineOp::Insert(line) => write_diff_line(
                    out,
                    format.output_indicator_new,
                    line,
                    diff_line_color(format.color, DiffLineColor::Insert),
                )?,
            }
            idx += 1;
        }
    }
    Ok(())
}

fn write_path_line<W: Write>(
    out: &mut W,
    color: bool,
    marker: &[u8],
    path: Option<(&str, &str)>,
) -> Result<()> {
    if color {
        out.write_all(b"\x1b[1m")?;
    }
    out.write_all(marker)?;
    out.write_all(b" ")?;
    if let Some((prefix, entry_path)) = path {
        out.write_all(prefix.as_bytes())?;
        out.write_all(entry_path.as_bytes())?;
    } else {
        out.write_all(b"/dev/null")?;
    }
    if color {
        out.write_all(b"\x1b[m")?;
    }
    out.write_all(b"\n")?;
    Ok(())
}

fn write_unified_range<W: Write>(out: &mut W, start: usize, count: usize) -> Result<()> {
    write!(out, "{start}")?;
    if count == 1 {
        return Ok(());
    }
    write!(out, ",{count}")?;
    Ok(())
}

fn write_patch_index_line<W: Write>(
    out: &mut W,
    color: bool,
    old_id: Option<&ObjectId>,
    new_id: Option<&ObjectId>,
    abbrev_len: usize,
    binary: bool,
    mode: Option<&str>,
) -> Result<()> {
    if color {
        out.write_all(b"\x1b[1m")?;
    }
    out.write_all(b"index ")?;
    write_patch_index_object_id(out, old_id, abbrev_len, binary)?;
    out.write_all(b"..")?;
    write_patch_index_object_id(out, new_id, abbrev_len, binary)?;
    if let Some(mode) = mode {
        out.write_all(b" ")?;
        out.write_all(mode.as_bytes())?;
    }
    if color {
        out.write_all(b"\x1b[m")?;
    }
    out.write_all(b"\n")?;
    Ok(())
}

fn write_patch_index_object_id<W: Write>(
    out: &mut W,
    id: Option<&ObjectId>,
    abbrev_len: usize,
    binary: bool,
) -> Result<()> {
    match (id, binary) {
        (Some(id), true) => id.write_hex_io(out).map_err(CliError::Io),
        (Some(id), false) => write_short_object_id_len(out, id, abbrev_len),
        (None, true) => out
            .write_all(ZERO_SHA1_HEX.as_bytes())
            .map_err(CliError::Io),
        (None, false) => write_zero_object_id_len(out, abbrev_len),
    }
}

fn write_short_object_id_len<W: Write>(out: &mut W, id: &ObjectId, len: usize) -> Result<()> {
    let len = len.min(id.hex_len());
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut buffer = [0_u8; 64];
    let mut written = 0usize;
    for byte in id.as_bytes() {
        if written == len {
            break;
        }
        buffer[written] = HEX[(byte >> 4) as usize];
        written += 1;
        if written == len {
            break;
        }
        buffer[written] = HEX[(byte & 0x0f) as usize];
        written += 1;
    }
    out.write_all(&buffer[..written])?;
    Ok(())
}

fn write_zero_object_id_len<W: Write>(out: &mut W, len: usize) -> Result<()> {
    if len <= ZERO_SHA1_HEX.len() {
        out.write_all(&ZERO_SHA1_HEX.as_bytes()[..len])?;
        return Ok(());
    }
    for _ in 0..len {
        out.write_all(b"0")?;
    }
    Ok(())
}

fn write_binary_files_line<W: Write>(
    out: &mut W,
    old_path: Option<(&str, &str)>,
    new_path: Option<(&str, &str)>,
) -> Result<()> {
    out.write_all(b"Binary files ")?;
    write_file_label(out, old_path)?;
    out.write_all(b" and ")?;
    write_file_label(out, new_path)?;
    out.write_all(b" differ")?;
    out.write_all(b"\n")?;
    Ok(())
}

fn write_file_label<W: Write>(out: &mut W, label: Option<(&str, &str)>) -> Result<()> {
    match label {
        Some((prefix, path)) => {
            out.write_all(prefix.as_bytes())?;
            out.write_all(path.as_bytes())?;
        }
        None => out.write_all(b"/dev/null")?,
    }
    Ok(())
}

pub(crate) fn hunk_ignored_by_matching_lines(
    ops: &[DiffLineOp<'_>],
    start: usize,
    end: usize,
    ignore_matching_lines: &[Regex],
) -> bool {
    if ignore_matching_lines.is_empty() {
        return false;
    }
    let mut saw_change = false;
    for op in &ops[start..end] {
        let line = match op {
            DiffLineOp::Equal(_) => continue,
            DiffLineOp::Delete(line) | DiffLineOp::Insert(line) => *line,
        };
        saw_change = true;
        if !ignore_matching_lines
            .iter()
            .any(|pattern| pattern.is_match(strip_trailing_lf(line)))
        {
            return false;
        }
    }
    saw_change
}

#[derive(Clone, Copy)]
pub(crate) enum DiffLineColor {
    Context,
    Delete,
    Insert,
}

pub(crate) fn diff_line_color(enabled: bool, color: DiffLineColor) -> Option<&'static [u8]> {
    if !enabled {
        return None;
    }
    Some(match color {
        DiffLineColor::Context => b"",
        DiffLineColor::Delete => b"\x1b[31m",
        DiffLineColor::Insert => b"\x1b[32m",
    })
}

pub(crate) fn write_patch_meta_line<W: Write>(
    out: &mut W,
    color: bool,
    line: std::fmt::Arguments<'_>,
) -> Result<()> {
    if color {
        out.write_all(b"\x1b[1m")?;
    }
    out.write_fmt(line)?;
    if color {
        out.write_all(b"\x1b[m")?;
    }
    writeln!(out)?;
    Ok(())
}

pub(crate) fn write_colored_line_body<W: Write>(out: &mut W, line: &[u8]) -> Result<()> {
    let (body, has_lf) = line
        .strip_suffix(b"\n")
        .map_or((line, false), |body| (body, true));
    out.write_all(body)?;
    out.write_all(b"\x1b[m")?;
    if has_lf {
        writeln!(out)?;
    }
    Ok(())
}

pub(crate) fn write_word_diff_line<W: Write>(
    out: &mut W,
    old: &[u8],
    new: &[u8],
    mode: WordDiffMode,
) -> Result<()> {
    let old_line = strip_trailing_lf(old);
    let new_line = strip_trailing_lf(new);
    let old_words = split_word_diff_tokens(old_line);
    let new_words = split_word_diff_tokens(new_line);
    let ops = diff_word_ops(&old_words, &new_words);
    for op in ops.iter().copied() {
        match (mode, op) {
            (WordDiffMode::Plain, DiffWordOp::Equal(token)) => out.write_all(token)?,
            (WordDiffMode::Plain, DiffWordOp::Delete(token))
                if token.iter().all(u8::is_ascii_whitespace) =>
            {
                out.write_all(token)?;
            }
            (WordDiffMode::Plain, DiffWordOp::Delete(token)) => {
                out.write_all(b"[-")?;
                out.write_all(token)?;
                out.write_all(b"-]")?;
            }
            (WordDiffMode::Plain, DiffWordOp::Insert(token))
                if token.iter().all(u8::is_ascii_whitespace) =>
            {
                out.write_all(token)?;
            }
            (WordDiffMode::Plain, DiffWordOp::Insert(token)) => {
                out.write_all(b"{+")?;
                out.write_all(token)?;
                out.write_all(b"+}")?;
            }
            (WordDiffMode::Color, DiffWordOp::Equal(token)) => out.write_all(token)?,
            (WordDiffMode::Color, DiffWordOp::Delete(token))
                if token.iter().all(u8::is_ascii_whitespace) =>
            {
                out.write_all(token)?;
            }
            (WordDiffMode::Color, DiffWordOp::Delete(token)) => {
                out.write_all(b"\x1b[31m")?;
                out.write_all(token)?;
                out.write_all(b"\x1b[m")?;
            }
            (WordDiffMode::Color, DiffWordOp::Insert(token))
                if token.iter().all(u8::is_ascii_whitespace) =>
            {
                out.write_all(token)?;
            }
            (WordDiffMode::Color, DiffWordOp::Insert(token)) => {
                out.write_all(b"\x1b[32m")?;
                out.write_all(token)?;
                out.write_all(b"\x1b[m")?;
            }
            (WordDiffMode::Porcelain, _) => {}
            (WordDiffMode::None, _) => {}
        }
    }
    if matches!(mode, WordDiffMode::Plain | WordDiffMode::Color) {
        writeln!(out)?;
    } else if mode == WordDiffMode::Porcelain {
        write_word_diff_porcelain_ops(out, &ops)?;
        writeln!(out, "~")?;
    }
    Ok(())
}

pub(crate) fn write_word_diff_change_block<W: Write>(
    out: &mut W,
    ops: &[DiffLineOp<'_>],
    start: usize,
    end: usize,
    mode: WordDiffMode,
) -> Result<usize> {
    let mut delete_end = start;
    while delete_end < end && matches!(ops[delete_end], DiffLineOp::Delete(_)) {
        delete_end += 1;
    }
    let mut insert_end = delete_end;
    while insert_end < end && matches!(ops[insert_end], DiffLineOp::Insert(_)) {
        insert_end += 1;
    }
    if delete_end == start || insert_end == delete_end {
        return Ok(start);
    }
    let delete_count = delete_end - start;
    let insert_count = insert_end - delete_end;
    let paired = delete_count.min(insert_count);
    for offset in 0..paired {
        let DiffLineOp::Delete(old_line) = ops[start + offset] else {
            return Err(CliError::Fatal {
                code: 128,
                message: "invalid word diff delete/insert block".into(),
            });
        };
        let DiffLineOp::Insert(new_line) = ops[delete_end + offset] else {
            return Err(CliError::Fatal {
                code: 128,
                message: "invalid word diff delete/insert block".into(),
            });
        };
        write_word_diff_line(out, old_line, new_line, mode)?;
    }
    for op in ops.iter().take(delete_end).skip(start + paired) {
        let DiffLineOp::Delete(line) = op else {
            return Err(CliError::Fatal {
                code: 128,
                message: "invalid word diff delete segment".into(),
            });
        };
        write_word_diff_delete_line(out, line, mode)?;
    }
    for op in ops.iter().take(insert_end).skip(delete_end + paired) {
        let DiffLineOp::Insert(line) = op else {
            return Err(CliError::Fatal {
                code: 128,
                message: "invalid word diff insert segment".into(),
            });
        };
        write_word_diff_insert_line(out, line, mode)?;
    }
    Ok(insert_end)
}

pub(crate) fn write_word_diff_delete_line<W: Write>(
    out: &mut W,
    line: &[u8],
    mode: WordDiffMode,
) -> Result<()> {
    match mode {
        WordDiffMode::Plain => {
            out.write_all(b"[-")?;
            out.write_all(strip_trailing_lf(line))?;
            out.write_all(b"-]")?;
            writeln!(out)?;
        }
        WordDiffMode::Color => {
            out.write_all(b"\x1b[31m")?;
            out.write_all(strip_trailing_lf(line))?;
            out.write_all(b"\x1b[m")?;
            writeln!(out)?;
        }
        WordDiffMode::Porcelain => {
            write_word_diff_porcelain_segment(out, b'-', strip_trailing_lf(line))?;
            writeln!(out, "~")?;
        }
        WordDiffMode::None => {}
    }
    Ok(())
}

pub(crate) fn write_word_diff_insert_line<W: Write>(
    out: &mut W,
    line: &[u8],
    mode: WordDiffMode,
) -> Result<()> {
    match mode {
        WordDiffMode::Plain => {
            out.write_all(b"{+")?;
            out.write_all(strip_trailing_lf(line))?;
            out.write_all(b"+}")?;
            writeln!(out)?;
        }
        WordDiffMode::Color => {
            out.write_all(b"\x1b[32m")?;
            out.write_all(strip_trailing_lf(line))?;
            out.write_all(b"\x1b[m")?;
            writeln!(out)?;
        }
        WordDiffMode::Porcelain => {
            write_word_diff_porcelain_segment(out, b'+', strip_trailing_lf(line))?;
            writeln!(out, "~")?;
        }
        WordDiffMode::None => {}
    }
    Ok(())
}

pub(crate) fn write_word_diff_porcelain_equal_line<W: Write>(
    out: &mut W,
    line: &[u8],
) -> Result<()> {
    write_word_diff_porcelain_segment(out, b' ', strip_trailing_lf(line))?;
    writeln!(out, "~")?;
    Ok(())
}

pub(crate) fn write_word_diff_porcelain_ops<W: Write>(
    out: &mut W,
    ops: &[DiffWordOp<'_>],
) -> Result<()> {
    let mut current_prefix: Option<u8> = None;
    let mut current = Vec::new();
    for op in ops {
        let (prefix, token) = match *op {
            DiffWordOp::Equal(token) => (b' ', token),
            DiffWordOp::Delete(token) | DiffWordOp::Insert(token)
                if token.iter().all(u8::is_ascii_whitespace) =>
            {
                (b' ', token)
            }
            DiffWordOp::Delete(token) => (b'-', token),
            DiffWordOp::Insert(token) => (b'+', token),
        };
        if Some(prefix) != current_prefix {
            if let Some(prefix) = current_prefix {
                write_word_diff_porcelain_segment(out, prefix, &current)?;
                current.clear();
            }
            current_prefix = Some(prefix);
        }
        current.extend_from_slice(token);
    }
    if let Some(prefix) = current_prefix {
        write_word_diff_porcelain_segment(out, prefix, &current)?;
    }
    Ok(())
}

pub(crate) fn write_word_diff_porcelain_segment<W: Write>(
    out: &mut W,
    prefix: u8,
    token: &[u8],
) -> Result<()> {
    if token.is_empty() {
        return Ok(());
    }
    out.write_all(&[prefix])?;
    out.write_all(token)?;
    writeln!(out)?;
    Ok(())
}

pub(crate) fn write_raw_diff_line<W: Write>(out: &mut W, line: &[u8]) -> Result<()> {
    out.write_all(line)?;
    if !line.ends_with(b"\n") {
        writeln!(out)?;
        writeln!(out, "\\ No newline at end of file")?;
    }
    Ok(())
}

pub(crate) fn strip_trailing_lf(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n").unwrap_or(line)
}

#[derive(Clone, Copy)]
pub(crate) enum DiffWordOp<'a> {
    Equal(&'a [u8]),
    Delete(&'a [u8]),
    Insert(&'a [u8]),
}

pub(crate) fn split_word_diff_tokens(line: &[u8]) -> Vec<&[u8]> {
    if line.is_empty() {
        return Vec::new();
    }
    let mut tokens = Vec::new();
    let mut start = 0usize;
    let mut current_is_space = line[0].is_ascii_whitespace();
    for (idx, byte) in line.iter().enumerate().skip(1) {
        let is_space = byte.is_ascii_whitespace();
        if is_space != current_is_space {
            tokens.push(&line[start..idx]);
            start = idx;
            current_is_space = is_space;
        }
    }
    tokens.push(&line[start..]);
    tokens
}

pub(crate) fn diff_word_ops<'a>(old: &[&'a [u8]], new: &[&'a [u8]]) -> Vec<DiffWordOp<'a>> {
    let (rows, cols, cell_count) = lcs_matrix_dimensions(old.len(), new.len());
    let mut lengths = vec![0usize; cell_count];
    for old_idx in (0..old.len()).rev() {
        for new_idx in (0..new.len()).rev() {
            lengths[lcs_matrix_idx(cols, old_idx, new_idx)] = if old[old_idx] == new[new_idx] {
                lengths[lcs_matrix_idx(cols, old_idx + 1, new_idx + 1)] + 1
            } else {
                lengths[lcs_matrix_idx(cols, old_idx + 1, new_idx)]
                    .max(lengths[lcs_matrix_idx(cols, old_idx, new_idx + 1)])
            };
        }
    }
    debug_assert_eq!(lengths.len(), rows * cols);

    let (mut old_idx, mut new_idx) = (0, 0);
    let mut ops = Vec::with_capacity(diff_ops_capacity(old.len(), new.len()));
    while old_idx < old.len() && new_idx < new.len() {
        if old[old_idx] == new[new_idx] {
            ops.push(DiffWordOp::Equal(old[old_idx]));
            old_idx += 1;
            new_idx += 1;
        } else if new_idx + 1 < new.len() && old[old_idx] == new[new_idx + 1] {
            ops.push(DiffWordOp::Insert(new[new_idx]));
            new_idx += 1;
        } else if (old_idx + 1 < old.len() && old[old_idx + 1] == new[new_idx])
            || lengths[lcs_matrix_idx(cols, old_idx + 1, new_idx)]
                >= lengths[lcs_matrix_idx(cols, old_idx, new_idx + 1)]
        {
            ops.push(DiffWordOp::Delete(old[old_idx]));
            old_idx += 1;
        } else {
            ops.push(DiffWordOp::Insert(new[new_idx]));
            new_idx += 1;
        }
    }
    while old_idx < old.len() {
        ops.push(DiffWordOp::Delete(old[old_idx]));
        old_idx += 1;
    }
    while new_idx < new.len() {
        ops.push(DiffWordOp::Insert(new[new_idx]));
        new_idx += 1;
    }
    ops
}

fn lcs_matrix_dimensions(left_len: usize, right_len: usize) -> (usize, usize, usize) {
    let rows = left_len
        .checked_add(1)
        .expect("diff input is too large to index");
    let cols = right_len
        .checked_add(1)
        .expect("diff input is too large to index");
    let cell_count = rows
        .checked_mul(cols)
        .expect("diff input is too large to index");
    (rows, cols, cell_count)
}

fn lcs_matrix_idx(cols: usize, row: usize, col: usize) -> usize {
    row * cols + col
}

fn diff_ops_capacity(old_len: usize, new_len: usize) -> usize {
    old_len
        .checked_add(new_len)
        .expect("diff input is too large to index")
}

fn diff_stat_row_initial_capacity(entry_count: usize) -> usize {
    entry_count.min(DIFF_STAT_ROW_INITIAL_CAPACITY_LIMIT).max(1)
}

fn hunk_header(
    old_lines: &[&[u8]],
    path: &str,
    old_start: usize,
    old_count: usize,
    unified_context: usize,
    header_cache: Option<&HunkHeaderCache>,
) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    if unified_context == 0 {
        return None;
    }
    if path.ends_with(".go") {
        if let Some(cache) = header_cache {
            return cache.get(old_start);
        }
        return go_hunk_header(old_lines, old_start);
    }
    if path.ends_with(".json") {
        return None;
    }
    if path.ends_with(".md") {
        if let Some(cache) = header_cache {
            return cache.get(old_start);
        }
        return markdown_hunk_header(old_lines, old_start);
    }
    if path.ends_with(".yml") || path.ends_with(".yaml") {
        if let Some(cache) = header_cache {
            return cache.get(old_start);
        }
        return yaml_hunk_header(old_lines, old_start);
    }
    if path == "Dockerfile" || path.ends_with("/Dockerfile") {
        return hunk_line_header(old_lines, old_start.saturating_sub(1));
    }
    if old_count == 0 {
        return None;
    }
    hunk_line_header(old_lines, old_start.saturating_sub(1))
}

#[derive(Debug)]
struct HunkHeaderCache {
    line_to_last_header: Vec<usize>,
    cached_headers: Vec<Option<String>>,
}

impl HunkHeaderCache {
    fn get(&self, old_start: usize) -> Option<String> {
        let target_start = old_start.saturating_sub(1);
        let line_idx = *self.line_to_last_header.get(target_start)?;
        if line_idx == NO_HEADER {
            return None;
        }
        self.cached_headers
            .get(line_idx)
            .and_then(Option::as_ref)
            .cloned()
    }
}

const NO_HEADER: usize = usize::MAX;

fn build_hunk_header_cache(
    old_lines: &[&[u8]],
    path: &str,
    unified_context: usize,
) -> Option<HunkHeaderCache> {
    if path.is_empty() || unified_context == 0 {
        return None;
    }
    if path.ends_with(".json") {
        return None;
    }
    if path == "Dockerfile" || path.ends_with("/Dockerfile") {
        return None;
    }
    if !(path.ends_with(".go")
        || path.ends_with(".md")
        || path.ends_with(".yml")
        || path.ends_with(".yaml"))
    {
        return None;
    }

    if path.ends_with(".go") {
        return Some(build_hunk_header_cache_kind(
            old_lines,
            cache_go_header_line,
        ));
    }

    if path.ends_with(".md") {
        return Some(build_hunk_header_cache_kind(
            old_lines,
            cache_markdown_header_line,
        ));
    }

    if path.ends_with(".yml") || path.ends_with(".yaml") {
        return Some(build_hunk_header_cache_kind(
            old_lines,
            cache_yaml_header_line,
        ));
    }

    None
}

fn build_hunk_header_cache_kind(
    old_lines: &[&[u8]],
    parser: fn(&[u8]) -> Option<String>,
) -> HunkHeaderCache {
    let mut line_to_last_header = vec![NO_HEADER; old_lines.len() + 1];
    let mut cached_headers = vec![None; old_lines.len()];
    let mut last: usize = NO_HEADER;

    for idx in 0..=old_lines.len() {
        if idx < old_lines.len() {
            line_to_last_header[idx] = last;
            if let Some(line) = parser(old_lines[idx]) {
                cached_headers[idx] = Some(line);
                last = idx;
            }
        } else {
            line_to_last_header[idx] = last;
        }
    }

    HunkHeaderCache {
        line_to_last_header,
        cached_headers,
    }
}

fn cache_go_header_line(line: &[u8]) -> Option<String> {
    if line.is_empty() || line.contains(&0) {
        return None;
    }
    let candidate = clean_hunk_header_line(line)?;
    if candidate.starts_with("package ")
        || candidate.starts_with("func ")
        || candidate.starts_with("var ")
        || candidate.starts_with("const ")
        || candidate.starts_with("type ")
    {
        Some(candidate)
    } else {
        None
    }
}

fn cache_markdown_header_line(line: &[u8]) -> Option<String> {
    let candidate = clean_hunk_header_line(line)?;
    if candidate == "---" || candidate.starts_with("**") || candidate.starts_with('#') {
        return None;
    }
    if candidate.starts_with('[') {
        return None;
    }
    Some(candidate)
}

fn cache_yaml_header_line(line: &[u8]) -> Option<String> {
    let line = String::from_utf8_lossy(line);
    let line = line.trim_end_matches(['\r', '\n']);
    if line.starts_with([' ', '\t']) {
        return None;
    }
    let candidate = line.trim();
    if candidate.is_empty() || candidate.starts_with('#') {
        return None;
    }
    if candidate.ends_with(':') {
        Some(truncate_hunk_header_line(candidate))
    } else {
        None
    }
}

pub(crate) fn go_hunk_header(old_lines: &[&[u8]], old_start: usize) -> Option<String> {
    for idx in (0..old_start.saturating_sub(1)).rev() {
        let Some(candidate) = clean_hunk_header_line(old_lines.get(idx)?) else {
            continue;
        };
        if candidate.starts_with("package ")
            || candidate.starts_with("func ")
            || candidate.starts_with("var ")
            || candidate.starts_with("const ")
            || candidate.starts_with("type ")
        {
            return Some(candidate);
        }
    }
    None
}

pub(crate) fn markdown_hunk_header(old_lines: &[&[u8]], old_start: usize) -> Option<String> {
    for idx in (0..old_start.saturating_sub(1)).rev() {
        let Some(candidate) = clean_hunk_header_line(old_lines.get(idx)?) else {
            continue;
        };
        if candidate == "---"
            || candidate.starts_with("**")
            || candidate.starts_with('#')
            || candidate.starts_with('[')
        {
            continue;
        }
        return Some(candidate);
    }
    None
}

pub(crate) fn yaml_hunk_header(old_lines: &[&[u8]], old_start: usize) -> Option<String> {
    for idx in (0..old_start.saturating_sub(1)).rev() {
        let line = String::from_utf8_lossy(old_lines.get(idx)?);
        let line = line.trim_end_matches(['\r', '\n']);
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let candidate = line.trim();
        if candidate.is_empty() || candidate.starts_with('#') {
            continue;
        };
        if candidate.ends_with(':') {
            return Some(truncate_hunk_header_line(candidate));
        }
    }
    None
}

pub(crate) fn hunk_line_header(old_lines: &[&[u8]], old_start: usize) -> Option<String> {
    let line = old_lines.get(old_start.checked_sub(1)?)?;
    clean_hunk_header_line(line)
}

pub(crate) fn clean_hunk_header_line(line: &[u8]) -> Option<String> {
    if line.is_empty() || line.contains(&0) {
        return None;
    }
    let line = String::from_utf8_lossy(line);
    let line = line.trim_end_matches(['\r', '\n']).trim();
    if line.is_empty() {
        None
    } else {
        Some(truncate_hunk_header_line(line))
    }
}

fn truncate_hunk_header_line(line: &str) -> String {
    const MAX_CHARS: usize = 80;

    let mut chars = line.chars();
    for _ in 0..MAX_CHARS {
        if chars.next().is_none() {
            return line.to_owned();
        }
    }

    let end = line.len() - chars.as_str().len();
    line[..end].to_owned()
}

pub(crate) fn unified_hunk_ranges(
    ops: &[DiffLineOp<'_>],
    context: usize,
    inter_hunk_context: usize,
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut current: Option<(usize, usize)> = None;
    for (change, op) in ops.iter().enumerate() {
        if !op.is_change() {
            continue;
        }
        let next_start = change.saturating_sub(context);
        let next_end = change
            .saturating_add(context)
            .saturating_add(1)
            .min(ops.len());
        match current {
            Some((start, end)) if next_start <= end.saturating_add(inter_hunk_context) => {
                current = Some((start, end.max(next_end)));
            }
            Some(range) => {
                ranges.push(range);
                current = Some((next_start, next_end));
            }
            None => current = Some((next_start, next_end)),
        }
    }
    if let Some(range) = current {
        ranges.push(range);
    }
    ranges
}

pub(crate) fn split_diff_lines(content: &[u8]) -> Vec<&[u8]> {
    if content.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::with_capacity(
        content.iter().filter(|byte| **byte == b'\n').count()
            + usize::from(!content.ends_with(b"\n")),
    );
    let mut start = 0usize;
    for (idx, byte) in content.iter().enumerate() {
        if *byte == b'\n' {
            lines.push(&content[start..=idx]);
            start = idx + 1;
        }
    }
    if start < content.len() {
        lines.push(&content[start..]);
    }
    lines
}

pub(crate) fn count_diff_lines(content: &[u8]) -> usize {
    if content.is_empty() {
        return 0;
    }
    content.iter().filter(|byte| **byte == b'\n').count() + usize::from(!content.ends_with(b"\n"))
}

#[derive(Clone, Copy)]
pub(crate) enum DiffLineOp<'a> {
    Equal(&'a [u8]),
    Delete(&'a [u8]),
    Insert(&'a [u8]),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffWhitespaceMode {
    None,
    AtEol,
    CrAtEol,
    Change,
    All,
}

impl DiffLineOp<'_> {
    fn is_change(&self) -> bool {
        !matches!(self, Self::Equal(_))
    }
}

pub(crate) fn diff_line_ops<'a>(old: &[&'a [u8]], new: &[&'a [u8]]) -> Vec<DiffLineOp<'a>> {
    diff_line_ops_with_whitespace(old, new, DiffWhitespaceMode::None)
}

pub(crate) fn diff_line_ops_with_whitespace<'a>(
    old: &[&'a [u8]],
    new: &[&'a [u8]],
    whitespace_mode: DiffWhitespaceMode,
) -> Vec<DiffLineOp<'a>> {
    let mut prefix_len = 0usize;
    while prefix_len < old.len()
        && prefix_len < new.len()
        && diff_lines_equal(old[prefix_len], new[prefix_len], whitespace_mode)
    {
        prefix_len += 1;
    }

    let mut suffix_len = 0usize;
    while prefix_len + suffix_len < old.len()
        && prefix_len + suffix_len < new.len()
        && diff_lines_equal(
            old[old.len() - suffix_len - 1],
            new[new.len() - suffix_len - 1],
            whitespace_mode,
        )
    {
        suffix_len += 1;
    }

    let old_inner_len = old.len() - prefix_len - suffix_len;
    let new_inner_len = new.len() - prefix_len - suffix_len;
    let ops_capacity = prefix_len
        .checked_add(suffix_len)
        .and_then(|len| len.checked_add(diff_ops_capacity(old_inner_len, new_inner_len)))
        .expect("diff input is too large to index");
    let mut ops = Vec::with_capacity(ops_capacity);
    ops.extend(new[..prefix_len].iter().copied().map(DiffLineOp::Equal));
    ops.extend(diff_line_ops_inner_with_whitespace(
        &old[prefix_len..old.len() - suffix_len],
        &new[prefix_len..new.len() - suffix_len],
        whitespace_mode,
    ));
    ops.extend(
        new[new.len() - suffix_len..]
            .iter()
            .copied()
            .map(DiffLineOp::Equal),
    );
    normalize_diff_blank_alignment_in_place(&mut ops);
    ops
}

const DIFF_MYERS_CELL_THRESHOLD: usize = 16_384;
const DIFF_MYERS_MIN_LINES: usize = 32;
type LineLcsLen = u16;

fn should_use_similar_myers(
    old_len: usize,
    new_len: usize,
    whitespace_mode: DiffWhitespaceMode,
) -> bool {
    matches!(whitespace_mode, DiffWhitespaceMode::None)
        && old_len > DIFF_MYERS_MIN_LINES
        && new_len > DIFF_MYERS_MIN_LINES
        && old_len
            .checked_mul(new_len)
            .is_none_or(|product| product > DIFF_MYERS_CELL_THRESHOLD)
}

fn diff_line_ops_myers<'a>(old: &[&'a [u8]], new: &[&'a [u8]]) -> Vec<DiffLineOp<'a>> {
    let mut ops = Vec::with_capacity(diff_ops_capacity(old.len(), new.len()));
    for segment in capture_diff_slices(Algorithm::Myers, old, new) {
        match segment {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                // Old/new indexes are guaranteed by `similar` to index into their own slices.
                // Prefer using the first slice for value reads to keep behavior consistent.
                if old_index == new_index {
                    ops.extend(
                        old[old_index..old_index + len]
                            .iter()
                            .copied()
                            .map(DiffLineOp::Equal),
                    );
                } else {
                    ops.extend(
                        new[new_index..new_index + len]
                            .iter()
                            .copied()
                            .map(DiffLineOp::Equal),
                    );
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                ops.extend(
                    old[old_index..old_index + old_len]
                        .iter()
                        .copied()
                        .map(DiffLineOp::Delete),
                );
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                ops.extend(
                    new[new_index..new_index + new_len]
                        .iter()
                        .copied()
                        .map(DiffLineOp::Insert),
                );
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                ops.extend(
                    old[old_index..old_index + old_len]
                        .iter()
                        .copied()
                        .map(DiffLineOp::Delete),
                );
                ops.extend(
                    new[new_index..new_index + new_len]
                        .iter()
                        .copied()
                        .map(DiffLineOp::Insert),
                );
            }
        }
    }
    normalize_diff_blank_alignment_in_place(&mut ops);
    ops
}

pub(crate) fn diff_line_ops_inner_with_whitespace<'a>(
    old: &[&'a [u8]],
    new: &[&'a [u8]],
    whitespace_mode: DiffWhitespaceMode,
) -> Vec<DiffLineOp<'a>> {
    if should_use_similar_myers(old.len(), new.len(), whitespace_mode) {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_diff_ops_myers");
        return diff_line_ops_myers(old, new);
    }
    let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_diff_ops_lcs");

    assert_line_lcs_len_fits(old.len(), new.len());
    let (rows, cols, cell_count) = lcs_matrix_dimensions(old.len(), new.len());
    let mut lengths = vec![0 as LineLcsLen; cell_count];
    for old_idx in (0..old.len()).rev() {
        for new_idx in (0..new.len()).rev() {
            lengths[lcs_matrix_idx(cols, old_idx, new_idx)] =
                if diff_lines_equal(old[old_idx], new[new_idx], whitespace_mode) {
                    lengths[lcs_matrix_idx(cols, old_idx + 1, new_idx + 1)] + 1
                } else {
                    lengths[lcs_matrix_idx(cols, old_idx + 1, new_idx)]
                        .max(lengths[lcs_matrix_idx(cols, old_idx, new_idx + 1)])
                };
        }
    }
    debug_assert_eq!(lengths.len(), rows * cols);

    let (mut old_idx, mut new_idx) = (0, 0);
    let mut ops = Vec::with_capacity(diff_ops_capacity(old.len(), new.len()));
    while old_idx < old.len() && new_idx < new.len() {
        if diff_lines_equal(old[old_idx], new[new_idx], whitespace_mode) {
            ops.push(DiffLineOp::Equal(new[new_idx]));
            old_idx += 1;
            new_idx += 1;
        } else if new_idx + 1 < new.len()
            && diff_lines_equal(old[old_idx], new[new_idx + 1], whitespace_mode)
        {
            ops.push(DiffLineOp::Insert(new[new_idx]));
            new_idx += 1;
        } else if (old_idx + 1 < old.len()
            && diff_lines_equal(old[old_idx + 1], new[new_idx], whitespace_mode))
            || lengths[lcs_matrix_idx(cols, old_idx + 1, new_idx)]
                >= lengths[lcs_matrix_idx(cols, old_idx, new_idx + 1)]
        {
            ops.push(DiffLineOp::Delete(old[old_idx]));
            old_idx += 1;
        } else {
            ops.push(DiffLineOp::Insert(new[new_idx]));
            new_idx += 1;
        }
    }
    while old_idx < old.len() {
        ops.push(DiffLineOp::Delete(old[old_idx]));
        old_idx += 1;
    }
    while new_idx < new.len() {
        ops.push(DiffLineOp::Insert(new[new_idx]));
        new_idx += 1;
    }
    normalize_diff_blank_alignment_in_place(&mut ops);
    ops
}

fn assert_line_lcs_len_fits(old_len: usize, new_len: usize) {
    let max_len = old_len.max(new_len);
    assert!(
        LineLcsLen::try_from(max_len).is_ok(),
        "line diff input is too large to store compact LCS lengths"
    );
}

pub(crate) fn normalize_diff_blank_alignment_in_place<'a>(ops: &mut Vec<DiffLineOp<'a>>) {
    let mut write = 0usize;
    let mut idx = 0usize;
    while idx < ops.len() {
        let delete_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Delete(_)) {
            idx += 1;
        }
        if delete_start == idx {
            if write != idx {
                ops[write] = ops[idx];
            }
            write += 1;
            idx += 1;
            continue;
        }

        if let Some(DiffLineOp::Equal(blank)) = ops.get(idx).copied()
            && is_blank_diff_line(blank)
        {
            let equal_idx = idx;
            idx += 1;
            let insert_start = idx;
            while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
                idx += 1;
            }
            if insert_start < idx
                && let DiffLineOp::Insert(inserted_blank) = ops[idx - 1]
                && inserted_blank == blank
            {
                let mut delete_cursor = delete_start;
                while delete_cursor < equal_idx {
                    let op = ops[delete_cursor];
                    ops[write] = op;
                    write += 1;
                    delete_cursor += 1;
                }
                ops[write] = DiffLineOp::Insert(inserted_blank);
                write += 1;
                let mut insert_cursor = insert_start;
                while insert_cursor < idx - 1 {
                    let op = ops[insert_cursor];
                    ops[write] = op;
                    write += 1;
                    insert_cursor += 1;
                }
                ops[write] = DiffLineOp::Equal(blank);
                write += 1;
                continue;
            }
            let mut cursor = delete_start;
            while cursor < idx {
                let op = ops[cursor];
                ops[write] = op;
                write += 1;
                cursor += 1;
            }
            continue;
        }

        let mut cursor = delete_start;
        while cursor < idx {
            let op = ops[cursor];
            ops[write] = op;
            write += 1;
            cursor += 1;
        }
    }
    ops.truncate(write);
}

pub(crate) fn is_blank_diff_line(line: &[u8]) -> bool {
    strip_trailing_lf(line).iter().all(u8::is_ascii_whitespace)
}

pub(crate) fn diff_lines_equal(
    old: &[u8],
    new: &[u8],
    whitespace_mode: DiffWhitespaceMode,
) -> bool {
    match whitespace_mode {
        DiffWhitespaceMode::None => old == new,
        DiffWhitespaceMode::AtEol => {
            trim_diff_line_trailing_space(old) == trim_diff_line_trailing_space(new)
        }
        DiffWhitespaceMode::CrAtEol => {
            trim_diff_line_trailing_cr(old) == trim_diff_line_trailing_cr(new)
        }
        DiffWhitespaceMode::Change => {
            collapse_diff_line_space(old) == collapse_diff_line_space(new)
        }
        DiffWhitespaceMode::All => remove_diff_line_space(old) == remove_diff_line_space(new),
    }
}

pub(crate) fn trim_diff_line_trailing_space(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    let has_lf = line.ends_with(b"\n");
    if has_lf {
        end -= 1;
    }
    while end > 0 && line[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &line[..end]
}

pub(crate) fn trim_diff_line_trailing_cr(line: &[u8]) -> &[u8] {
    if let Some(without_lf) = line.strip_suffix(b"\n") {
        without_lf.strip_suffix(b"\r").unwrap_or(without_lf)
    } else {
        line.strip_suffix(b"\r").unwrap_or(line)
    }
}

pub(crate) fn collapse_diff_line_space(line: &[u8]) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(line.len());
    let mut in_space = false;
    for byte in strip_trailing_lf(line).iter().copied() {
        if byte.is_ascii_whitespace() {
            in_space = true;
        } else {
            if in_space && !normalized.is_empty() {
                normalized.push(b' ');
            }
            normalized.push(byte);
            in_space = false;
        }
    }
    normalized
}

pub(crate) fn remove_diff_line_space(line: &[u8]) -> Vec<u8> {
    strip_trailing_lf(line)
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect()
}

pub(crate) fn write_diff_line<W: Write>(
    out: &mut W,
    prefix: Option<u8>,
    line: &[u8],
    color: Option<&'static [u8]>,
) -> Result<()> {
    if let Some(color) = color {
        match (color, prefix) {
            (b"\x1b[32m", Some(b'+')) => {
                out.write_all(color)?;
                out.write_all(b"+\x1b[m")?;
                out.write_all(color)?;
                write_colored_line_body(out, line)?;
            }
            (b"", Some(prefix)) => {
                out.write_all(&[prefix])?;
                write_colored_line_body(out, line)?;
            }
            (b"", None) => write_colored_line_body(out, line)?,
            (color, Some(prefix)) => {
                out.write_all(color)?;
                out.write_all(&[prefix])?;
                write_colored_line_body(out, line)?;
            }
            (color, None) => {
                out.write_all(color)?;
                write_colored_line_body(out, line)?;
            }
        }
    } else {
        if let Some(prefix) = prefix {
            let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_plain_line.prefix");
            out.write_all(&[prefix])?;
        }
        {
            let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_plain_line.body");
            out.write_all(line)?;
        }
    }
    if !line.ends_with(b"\n") {
        let _trace = phase_trace("format_patch.write_tree_diff.entry_hunk_plain_line.no_newline");
        writeln!(out)?;
        writeln!(out, "\\ No newline at end of file")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff_line_op_tags(ops: &[DiffLineOp<'_>]) -> Vec<(u8, Vec<u8>)> {
        ops.iter()
            .map(|op| match op {
                DiffLineOp::Equal(line) => (b' ', line.to_vec()),
                DiffLineOp::Delete(line) => (b'-', line.to_vec()),
                DiffLineOp::Insert(line) => (b'+', line.to_vec()),
            })
            .collect()
    }

    fn diff_word_op_tags(ops: &[DiffWordOp<'_>]) -> Vec<(u8, Vec<u8>)> {
        ops.iter()
            .map(|op| match op {
                DiffWordOp::Equal(token) => (b' ', token.to_vec()),
                DiffWordOp::Delete(token) => (b'-', token.to_vec()),
                DiffWordOp::Insert(token) => (b'+', token.to_vec()),
            })
            .collect()
    }

    #[test]
    fn diff_stat_row_initial_capacity_is_bounded() {
        assert_eq!(
            diff_stat_row_initial_capacity(usize::MAX),
            DIFF_STAT_ROW_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(diff_stat_row_initial_capacity(2), 2);
        assert_eq!(diff_stat_row_initial_capacity(0), 1);
    }

    #[test]
    fn compact_stat_path_keeps_tail_without_collecting_all_components() {
        assert_eq!(
            compact_stat_path("src/runtime/deep/path/file.rs", 18),
            ".../path/file.rs"
        );
        assert_eq!(compact_stat_path("longfilename.rs", 8), "...me.rs");
        assert_eq!(compact_stat_path("alpha/beta/gamma.rs", 12), ".../gamma.rs");
    }

    #[test]
    fn diff_pairs_nul_parser_streams_rename_fields_across_batches() {
        let blob_a = "1111111111111111111111111111111111111111";
        let blob_b = "2222222222222222222222222222222222222222";
        let input = format!(
            ":100644 100644 {blob_a} {blob_b} R100\0old.txt\0new.txt\0\0:000000 100644 {blob_a} {blob_b} A\0added.txt\0"
        );

        let batches = parse_diff_pairs_batches(input.as_bytes(), true).expect("diff pairs");

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].entries.len(), 1);
        assert_eq!(
            batches[0].entries[0].old_path.as_deref(),
            Some(&b"old.txt"[..])
        );
        assert_eq!(batches[0].entries[0].path, b"new.txt");
        assert_eq!(batches[1].entries.len(), 1);
        assert_eq!(batches[1].entries[0].path, b"added.txt");
    }

    #[test]
    fn diff_pairs_line_parser_streams_rename_fields_across_batches() {
        let blob_a = "1111111111111111111111111111111111111111";
        let blob_b = "2222222222222222222222222222222222222222";
        let input = format!(
            ":100644 100644 {blob_a} {blob_b} R100\told.txt\tnew.txt\n\n:000000 100644 {blob_a} {blob_b} A\tadded.txt\n"
        );

        let batches = parse_diff_pairs_batches(input.as_bytes(), false).expect("diff pairs");

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].entries.len(), 1);
        assert_eq!(
            batches[0].entries[0].old_path.as_deref(),
            Some(&b"old.txt"[..])
        );
        assert_eq!(batches[0].entries[0].path, b"new.txt");
        assert_eq!(batches[1].entries.len(), 1);
        assert_eq!(batches[1].entries[0].path, b"added.txt");
    }

    #[test]
    fn lcs_line_bytes_counts_common_line_bytes_with_single_row_dp() {
        let left = split_diff_lines(b"alpha\nbeta\ngamma\n");
        let right = split_diff_lines(b"beta\ndelta\ngamma\n");

        assert_eq!(
            lcs_line_bytes(&left, &right),
            b"beta\n".len() + b"gamma\n".len()
        );
    }

    #[test]
    fn diff_line_ops_preserves_insert_delete_tie_breaks_with_flat_lcs_matrix() {
        let old_lines = split_diff_lines(b"a\nb\nc\n");
        let new_lines = split_diff_lines(b"b\nx\nc\n");
        let ops =
            diff_line_ops_inner_with_whitespace(&old_lines, &new_lines, DiffWhitespaceMode::None);

        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (b'-', b"a\n".to_vec()),
                (b' ', b"b\n".to_vec()),
                (b'+', b"x\n".to_vec()),
                (b' ', b"c\n".to_vec()),
            ]
        );
    }

    #[test]
    fn diff_line_ops_myers_preserves_insert_delete_tie_breaks() {
        let old_lines = split_diff_lines(b"a\nb\nc\n");
        let new_lines = split_diff_lines(b"b\nx\nc\n");
        let ops = diff_line_ops_myers(&old_lines, &new_lines);

        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (b'-', b"a\n".to_vec()),
                (b' ', b"b\n".to_vec()),
                (b'+', b"x\n".to_vec()),
                (b' ', b"c\n".to_vec()),
            ]
        );
    }

    #[test]
    fn diff_word_ops_keeps_current_word_alignment_shape() {
        let old = split_word_diff_tokens(b"alpha beta gamma");
        let new = split_word_diff_tokens(b"beta delta gamma");
        let ops = diff_word_ops(&old, &new);

        assert_eq!(
            diff_word_op_tags(&ops),
            vec![
                (b'-', b"alpha".to_vec()),
                (b'+', b"beta".to_vec()),
                (b' ', b" ".to_vec()),
                (b'-', b"beta".to_vec()),
                (b'+', b"delta".to_vec()),
                (b' ', b" ".to_vec()),
                (b' ', b"gamma".to_vec()),
            ]
        );
    }

    #[test]
    fn is_binary_content_matches_git_first_bytes_heuristic() {
        assert!(is_binary_content(b"alpha\0beta\n"));

        let mut late_nul = vec![b'a'; BINARY_DETECTION_BYTES];
        late_nul.push(0);
        late_nul.extend_from_slice(b"beta\n");
        assert!(!is_binary_content(&late_nul));
    }

    #[test]
    fn unified_hunk_ranges_merge_touching_context_spans() {
        let old_lines = split_diff_lines(b"0\n1\n2\n3\n4\n5\n6\n");
        let new_lines = split_diff_lines(b"0\none\n2\nthree\n4\n5\nsix\n");
        let ops = diff_line_ops(&old_lines, &new_lines);

        assert_eq!(unified_hunk_ranges(&ops, 1, 0), vec![(0, 10)]);
        assert_eq!(unified_hunk_ranges(&ops, 1, 1), vec![(0, 10)]);
    }

    #[test]
    fn unified_full_file_hunk_fast_path_formats_added_file() {
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            b"",
            b"alpha\nbeta\n",
            "notes.txt",
            HunkFormatOptions::default(),
        )
        .expect("write added hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "@@ -0,0 +1,2 @@\n+alpha\n+beta\n"
        );
    }

    #[test]
    fn unified_full_file_hunk_fast_path_formats_deleted_file_without_newline() {
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            b"alpha",
            b"",
            "notes.txt",
            HunkFormatOptions::default(),
        )
        .expect("write deleted hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "@@ -1 +0,0 @@\n-alpha\n\\ No newline at end of file\n"
        );
    }

    #[test]
    fn split_diff_lines_preserves_trailing_and_non_trailing_final_lines() {
        assert_eq!(split_diff_lines(b""), Vec::<&[u8]>::new());
        assert_eq!(
            split_diff_lines(b"alpha\nbeta\n"),
            vec![&b"alpha\n"[..], &b"beta\n"[..]]
        );
        assert_eq!(
            split_diff_lines(b"alpha\nbeta"),
            vec![&b"alpha\n"[..], &b"beta"[..]]
        );
        assert_eq!(split_diff_lines(b"alpha"), vec![&b"alpha"[..]]);
    }

    #[test]
    fn commit_subject_view_uses_first_line_only() {
        let body = b"subject line\n\nsecond line\nthird line\n";
        assert_eq!(commit_subject_view(body), "subject line");
        let unicode = b"subj\x80ct\n\nbody\n";
        assert_eq!(commit_subject_view(unicode), "subj\u{fffd}ct");
    }

    #[test]
    fn commit_message_body_returns_trimmed_body_text() {
        assert_eq!(commit_message_body(b"subject\n\nbody\n"), "body");
        assert_eq!(commit_message_body(b"subject\n\nbody"), "body");
        assert_eq!(commit_message_body(b"subject\n\n"), "");
        assert_eq!(commit_message_body(b"subject\nno-body"), "");
    }

    #[test]
    fn clean_hunk_header_line_keeps_short_lines_without_rebuilding_chars() {
        assert_eq!(
            clean_hunk_header_line("  короткий заголовок  \n".as_bytes()),
            Some("короткий заголовок".to_owned())
        );
    }

    #[test]
    fn clean_hunk_header_line_truncates_at_80_chars_on_char_boundary() {
        let line = format!("  {}  \n", "ї".repeat(90));
        assert_eq!(
            clean_hunk_header_line(line.as_bytes()),
            Some("ї".repeat(80))
        );
    }
}
