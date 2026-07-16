use super::*;
use rustc_hash::FxHashMap;
use similar::{Algorithm, DiffOp, DiffTag, capture_diff_slices};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::ops::Range;
use std::sync::Arc;

use zmin_git_core::GitObjectStore;

const DIFF_STAT_ROW_INITIAL_CAPACITY_LIMIT: usize = 8192;
const PARALLEL_DIFF_STAT_MIN_ENTRIES: usize = 16;
const PARALLEL_DIFF_STAT_SHARED_PRELOAD_MIN_ENTRIES: usize = 16;
const PARALLEL_DIFF_STAT_MAX_WORKERS: usize = 8;
const DIFF_LINE_COUNT_INTERN_MIN_LINES: usize = 512;
const ZERO_SHA1_HEX: &str = "0000000000000000000000000000000000000000";
const BREAK_REWRITE_MIN_LINES: usize = 100;

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
) -> Result<Vec<zmin_git_core::IndexDiffEntry>> {
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
    entry: &zmin_git_core::IndexDiffEntry,
    pathspecs: &[Vec<u8>],
) -> bool {
    pathspec_matches(&entry.path, pathspecs)
        || entry
            .old_path
            .as_deref()
            .is_some_and(|path| pathspec_matches(path, pathspecs))
}

#[derive(Clone, Copy)]
pub(crate) struct PickaxeOptions<'a> {
    pub(crate) string: Option<&'a str>,
    pub(crate) regex: Option<&'a str>,
    pub(crate) regex_mode: bool,
    pub(crate) all: bool,
}
impl PickaxeOptions<'_> {
    pub(crate) fn enabled(&self) -> bool {
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
    entries: Vec<zmin_git_core::IndexDiffEntry>,
    options: PickaxeOptions<'_>,
) -> Result<Vec<zmin_git_core::IndexDiffEntry>> {
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
    entries: Vec<zmin_git_core::IndexDiffEntry>,
    options: SimilarityDetectionOptions,
) -> Result<Vec<zmin_git_core::IndexDiffEntry>> {
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
    entries: Vec<zmin_git_core::IndexDiffEntry>,
    threshold: u8,
) -> Result<Vec<zmin_git_core::IndexDiffEntry>> {
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
        out.push(zmin_git_core::IndexDiffEntry {
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
    entries: Vec<zmin_git_core::IndexDiffEntry>,
    threshold: u8,
    find_copies_harder: bool,
) -> Result<Vec<zmin_git_core::IndexDiffEntry>> {
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
    old_entry: &zmin_git_core::IndexDiffEntry,
    new_entry: &zmin_git_core::IndexDiffEntry,
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
    entries: Vec<zmin_git_core::IndexDiffEntry>,
    threshold: Option<u8>,
) -> Result<Vec<zmin_git_core::IndexDiffEntry>> {
    let Some(threshold) = threshold else {
        return Ok(entries);
    };
    entries
        .into_iter()
        .map(|mut entry| {
            if entry.status == IndexDiffStatus::Modified {
                let score = diff_entry_similarity_score(context, &entry, &entry)?;
                let dissimilarity = 100_u8.saturating_sub(score);
                if dissimilarity >= threshold
                    && diff_entry_break_rewrite_large_enough(context, &entry)?
                {
                    entry.similarity = Some(dissimilarity);
                }
            }
            Ok(entry)
        })
        .collect()
}

fn diff_entry_break_rewrite_large_enough(
    context: &DiffIndexContext<'_>,
    entry: &zmin_git_core::IndexDiffEntry,
) -> Result<bool> {
    let Some(old_index_entry) = find_index_entry(context.old_index, &entry.path) else {
        return Ok(false);
    };
    let Some(new_index_entry) = find_index_entry(context.new_index, &entry.path) else {
        return Ok(false);
    };
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
    Ok(
        diff_content_line_count(&old_content) >= BREAK_REWRITE_MIN_LINES
            && diff_content_line_count(&new_content) >= BREAK_REWRITE_MIN_LINES,
    )
}

fn diff_content_line_count(content: &[u8]) -> usize {
    if content.is_empty() {
        return 0;
    }
    let newline_count = content.iter().filter(|byte| **byte == b'\n').count();
    if content.ends_with(b"\n") {
        newline_count
    } else {
        newline_count + 1
    }
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
    let old_content = normalize_similarity_crlf(old_content);
    let new_content = normalize_similarity_crlf(new_content);
    let old_lines = split_diff_lines(old_content.as_ref());
    let new_lines = split_diff_lines(new_content.as_ref());
    if old_lines.is_empty() && new_lines.is_empty() {
        return 100;
    }
    let common = lcs_line_bytes(&old_lines, &new_lines);
    ((common * 100) / old_content.len().max(new_content.len())) as u8
}

fn normalize_similarity_crlf(content: &[u8]) -> Cow<'_, [u8]> {
    if !content.windows(2).any(|window| window == b"\r\n") {
        return Cow::Borrowed(content);
    }

    let mut normalized = Vec::with_capacity(content.len());
    let mut index = 0;
    while index < content.len() {
        if content[index] == b'\r' && content.get(index + 1) == Some(&b'\n') {
            index += 1;
        }
        normalized.push(content[index]);
        index += 1;
    }
    Cow::Owned(normalized)
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
    entries: Vec<zmin_git_core::IndexDiffEntry>,
    order_file: Option<&Path>,
) -> Result<Vec<zmin_git_core::IndexDiffEntry>> {
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
    entry: &zmin_git_core::IndexDiffEntry,
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
    mut entries: Vec<zmin_git_core::IndexDiffEntry>,
    skip_to: Option<&str>,
    rotate_to: Option<&str>,
) -> Vec<zmin_git_core::IndexDiffEntry> {
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

pub(crate) fn diff_entry_matches_name(entry: &zmin_git_core::IndexDiffEntry, target: &str) -> bool {
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
    pub(crate) old_root_has_files: bool,
    pub(crate) new_root_has_files: bool,
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
    let mut paths = options.paths.clone();
    if let Some(separator) = paths.iter().position(|path| path == Path::new("--")) {
        paths.remove(separator);
    }
    let paths = &paths;
    if paths.len() != 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "diff --no-index requires exactly two paths".into(),
        });
    }
    let left = absolute_path_from_arg(&paths[0])?;
    let right = absolute_path_from_arg(&paths[1])?;
    for (original, path) in [(&paths[0], &left), (&paths[1], &right)] {
        if !is_null_diff_path(original) && !path_exists(path) {
            return Err(CliError::Stderr {
                code: 1,
                text: format!(
                    "error: Could not access '{}'\n",
                    no_index_display_path(original)
                ),
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
            text: format!(
                "error: Could not access '{}'\n",
                no_index_display_path(&missing)
            ),
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
    let left_content = no_index_file_content(left_arg, left)?;
    let right_content = no_index_file_content(right_arg, right)?;
    if left_content == right_content {
        return Ok(Vec::new());
    }
    let left_is_null = is_null_diff_path(left_arg);
    let right_is_null = is_null_diff_path(right_arg);
    let left_display = if left_is_null {
        no_index_display_path(right_arg)
    } else {
        no_index_display_path(left_arg)
    };
    let right_display = if right_is_null {
        no_index_display_path(left_arg)
    } else {
        no_index_display_path(right_arg)
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
        old_root_has_files: !left_is_null,
        new_root_has_files: !right_is_null,
    }])
}

pub(crate) fn collect_no_index_directory_entries(
    left_arg: &Path,
    left: &Path,
    right_arg: &Path,
    right: &Path,
) -> Result<Vec<NoIndexDiffEntry>> {
    let mut left_rels = BTreeSet::new();
    if left.is_dir() {
        collect_no_index_relative_files(left, left, &mut left_rels)?;
    }
    let mut right_rels = BTreeSet::new();
    if right.is_dir() {
        collect_no_index_relative_files(right, right, &mut right_rels)?;
    }
    let left_root_has_files = !left_rels.is_empty();
    let right_root_has_files = !right_rels.is_empty();
    let rels = left_rels
        .union(&right_rels)
        .cloned()
        .collect::<BTreeSet<_>>();
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
        let rel_display = no_index_display_path(&rel);
        let left_display = no_index_display_path(&left_arg.join(&rel));
        let right_display = no_index_display_path(&right_arg.join(&rel));
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
            old_root_has_files: left_root_has_files,
            new_root_has_files: right_root_has_files,
        });
    }
    Ok(entries)
}

fn no_index_display_path(path: &Path) -> String {
    let value = path.display().to_string();
    #[cfg(windows)]
    {
        value.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        value
    }
}

fn no_index_file_content(arg: &Path, path: &Path) -> Result<Vec<u8>> {
    if is_null_diff_path(arg) {
        Ok(Vec::new())
    } else {
        Ok(fs::read(path)?)
    }
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
                    options.ignore_blank_lines,
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
        let abbrev_len = parse_diff_abbrev_len(options.abbrev.as_deref(), options.no_abbrev)?;
        print_no_index_raw(&rows, abbrev_len);
        return Err(CliError::Exit(1));
    }
    let stat_rows = rows
        .iter()
        .map(|(entry, is_binary, insertions, deletions)| DiffStatRow {
            path: entry.stat_path.clone(),
            compact_summary: None,
            old_bytes: entry.old_content.len(),
            new_bytes: entry.new_content.len(),
            insertions: *insertions,
            deletions: *deletions,
            dirstat_changes: if *is_binary {
                entry.old_content.len().max(entry.new_content.len())
            } else if entry.old_content == entry.new_content {
                0
            } else {
                diff_dirstat_changes(&entry.old_content, &entry.new_content, false)
            },
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
        if options.summary {
            print_no_index_summary(&rows);
        }
        println!();
    } else if options.patch_with_raw {
        let abbrev_len = parse_diff_abbrev_len(options.abbrev.as_deref(), options.no_abbrev)?;
        print_no_index_raw(&rows, abbrev_len);
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
    std::mem::swap(&mut entry.old_root_has_files, &mut entry.new_root_has_files);
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

pub(crate) fn print_no_index_raw(
    rows: &[(&NoIndexDiffEntry, bool, usize, usize)],
    abbrev_len: Option<usize>,
) {
    let abbrev_len = abbrev_len.unwrap_or(7);
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
        let old_hash = if entry.status == IndexDiffStatus::Deleted && entry.new_root_has_files {
            no_index_raw_blob_hash(&entry.old_content, abbrev_len)
        } else {
            diff_raw_zero_object_id_len(abbrev_len)
        };
        let new_hash = if entry.status == IndexDiffStatus::Added && entry.old_root_has_files {
            no_index_raw_blob_hash(&entry.new_content, abbrev_len)
        } else {
            diff_raw_zero_object_id_len(abbrev_len)
        };
        println!(
            ":{old_mode} {new_mode} {old_hash} {new_hash} {}\t{}",
            entry.status.name_status(),
            entry.name_status_path
        );
    }
}

fn no_index_raw_blob_hash(content: &[u8], abbrev_len: usize) -> String {
    let id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, content);
    diff_raw_object_id_len(&id, abbrev_len)
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
                    word_diff_regex: None,
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
                    ignore_blank_lines: options.ignore_blank_lines,
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
    entries: &[zmin_git_core::IndexDiffEntry],
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

#[derive(Clone)]
pub(crate) struct DiffRenderOptions {
    pub(crate) stat: bool,
    pub(crate) patch_with_raw: bool,
    pub(crate) patch_with_stat: bool,
    pub(crate) compact_summary: bool,
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
    pub(crate) word_diff_regex: Option<String>,
    pub(crate) patch_abbrev_len: Option<usize>,
    pub(crate) old_prefix: String,
    pub(crate) new_prefix: String,
    pub(crate) unified_context: usize,
    pub(crate) inter_hunk_context: usize,
    pub(crate) output_indicator_new: Option<u8>,
    pub(crate) output_indicator_old: Option<u8>,
    pub(crate) output_indicator_context: Option<u8>,
    pub(crate) ignore_matching_lines: Vec<Regex>,
    pub(crate) ignore_blank_lines: bool,
    pub(crate) whitespace_mode: DiffWhitespaceMode,
    pub(crate) relative_prefix: Option<Vec<u8>>,
    pub(crate) text: bool,
    pub(crate) irreversible_delete: bool,
    pub(crate) submodule_format: SubmoduleDiffFormat,
    pub(crate) color_mode: DiffColorMode,
    pub(crate) old_source: DiffSideSource,
    pub(crate) new_source: DiffSideSource,
    pub(crate) line_prefix: Option<String>,
}

impl DiffRenderOptions {
    pub(crate) fn validate_format(&self, include_patch: bool) -> Result<()> {
        let summary_is_addon = self.summary && (self.stat || self.patch_with_stat);
        if [
            self.stat,
            self.patch_with_raw,
            self.patch_with_stat,
            self.numstat,
            self.shortstat,
            self.raw,
            self.summary && !summary_is_addon,
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
    pub(crate) fn enabled(self) -> bool {
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
    entry: &zmin_git_core::IndexDiffEntry,
    old_index: &GitIndex,
    new_index: &GitIndex,
) -> bool {
    find_index_entry(old_index, diff_entry_old_path(entry))
        .is_some_and(|entry| entry.mode == IndexMode::Gitlink)
        || find_index_entry(new_index, &entry.path)
            .is_some_and(|entry| entry.mode == IndexMode::Gitlink)
}

pub(crate) fn filter_ignored_submodule_entries(
    entries: Vec<zmin_git_core::IndexDiffEntry>,
    old_index: &GitIndex,
    new_index: &GitIndex,
    mode: IgnoreSubmodulesMode,
) -> Vec<zmin_git_core::IndexDiffEntry> {
    if mode != IgnoreSubmodulesMode::All {
        return entries;
    }
    entries
        .into_iter()
        .filter(|entry| !diff_entry_is_gitlink(entry, old_index, new_index))
        .collect()
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
    entries: &[zmin_git_core::IndexDiffEntry],
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
        ignore_blank_lines: options.ignore_blank_lines,
        compact_summary: options.compact_summary,
        color: options.color_mode.enabled(),
    };
    let raw_options = RawPrintOptions {
        abbrev_len: options.raw_abbrev_len,
        relative_prefix: options.relative_prefix.as_deref(),
        nul_terminated: options.nul_terminated,
    };
    let visible_entries;
    let entries = if diff_render_filters_entries(&options) {
        visible_entries = diff_visible_entries(&context, entries, stat_options)?;
        visible_entries.as_slice()
    } else {
        entries
    };
    let has_diff = !entries.is_empty();
    if !options.quiet && !options.no_patch {
        if options.patch_with_stat {
            print_stat_entries(&context, entries, stat_options)?;
            if options.summary {
                print_summary_entries(
                    old_index,
                    new_index,
                    entries,
                    options.relative_prefix.as_deref(),
                )?;
            }
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
            if options.summary {
                print_summary_entries(
                    old_index,
                    new_index,
                    entries,
                    options.relative_prefix.as_deref(),
                )?;
            }
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

fn diff_render_filters_entries(options: &DiffRenderOptions) -> bool {
    options.whitespace_mode != DiffWhitespaceMode::None
        || !options.ignore_matching_lines.is_empty()
        || options.ignore_blank_lines
}

fn diff_visible_entries(
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
) -> Result<Vec<zmin_git_core::IndexDiffEntry>> {
    let mut visible = Vec::with_capacity(entries.len());
    for entry in entries {
        if diff_entry_visible_with_options(context, entry, options)? {
            visible.push(entry.clone());
        }
    }
    Ok(visible)
}

fn diff_entry_visible_with_options(
    context: &DiffIndexContext<'_>,
    entry: &zmin_git_core::IndexDiffEntry,
    options: DiffStatOptions<'_>,
) -> Result<bool> {
    let row = match diff_stat_row_with_whitespace(context, entry, options, None, false) {
        Ok(row) => row,
        Err(CliError::Fatal { message, .. })
            if message.contains("is not a file")
                && context.new_source == DiffSideSource::WorktreeOrIndex =>
        {
            return Ok(true);
        }
        Err(error) => return Err(error),
    };
    if row.binary || row.insertions + row.deletions > 0 || row.compact_summary.is_some() {
        return Ok(true);
    }
    Ok(diff_entry_has_metadata_change(context, entry))
}

fn diff_entry_has_metadata_change(
    context: &DiffIndexContext<'_>,
    entry: &zmin_git_core::IndexDiffEntry,
) -> bool {
    if matches!(
        entry.status,
        IndexDiffStatus::Added
            | IndexDiffStatus::Copied
            | IndexDiffStatus::Deleted
            | IndexDiffStatus::Renamed
    ) {
        return true;
    }
    let old_entry = find_index_entry(context.old_index, diff_entry_old_path(entry));
    let new_entry = find_index_entry(context.new_index, &entry.path);
    matches!(
        (old_entry.map(|entry| entry.mode), new_entry.map(|entry| entry.mode)),
        (Some(old_mode), Some(new_mode)) if old_mode != new_mode
    )
}

pub(crate) fn print_render_patch_entries(
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
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
            word_diff_regex: options.word_diff_regex.clone(),
            abbrev_len: options.patch_abbrev_len,
            old_prefix: options.old_prefix.clone(),
            new_prefix: options.new_prefix.clone(),
            unified_context: options.unified_context,
            inter_hunk_context: options.inter_hunk_context,
            output_indicator_new: options.output_indicator_new,
            output_indicator_old: options.output_indicator_old,
            output_indicator_context: options.output_indicator_context,
            ignore_matching_lines: options.ignore_matching_lines.clone(),
            ignore_blank_lines: options.ignore_blank_lines,
            whitespace_mode: options.whitespace_mode,
            relative_prefix: options.relative_prefix.clone(),
            text: options.text,
            binary: options.binary,
            irreversible_delete: options.irreversible_delete,
            submodule_format: options.submodule_format,
            color_mode,
            emit_hunk_headers: true,
            line_prefix: options.line_prefix.clone(),
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
        Arc::<[zmin_git_core::TreeEntry]>::from(Vec::new().into_boxed_slice())
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

pub(crate) fn reverse_index_diff_entries(entries: &mut [zmin_git_core::IndexDiffEntry]) {
    for entry in entries {
        entry.status = match entry.status {
            IndexDiffStatus::Added => IndexDiffStatus::Deleted,
            IndexDiffStatus::Deleted => IndexDiffStatus::Added,
            status => status,
        };
        if let Some(old_path) = entry.old_path.as_mut() {
            std::mem::swap(&mut entry.path, old_path);
        }
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
    } else if options.summary {
        print_root_tree_summary_entries(store, &entries, relative_prefix)?;
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

fn print_root_tree_summary_entries(
    store: &LooseObjectStore,
    entries: &[RootTreeDiffEntry],
    relative_prefix: Option<&[u8]>,
) -> Result<()> {
    let tree_cache = TreeObjectCache::new(store);
    for entry in entries {
        match entry.status {
            IndexDiffStatus::Added => print_root_tree_summary_create(
                &tree_cache,
                entry.new_mode,
                entry.new_id.as_ref(),
                &entry.path,
                relative_prefix,
            )?,
            IndexDiffStatus::Deleted => print_root_tree_summary_delete(
                &tree_cache,
                entry.old_mode,
                entry.old_id.as_ref(),
                &entry.path,
                relative_prefix,
            )?,
            IndexDiffStatus::Modified if entry.old_mode != entry.new_mode => {
                let old_mode = entry.old_mode.map(tree_mode_octal).unwrap_or("000000");
                let new_mode = entry.new_mode.map(tree_mode_octal).unwrap_or("000000");
                println!(
                    " mode change {old_mode} => {new_mode} {}",
                    diff_display_path(&entry.path, relative_prefix)
                );
            }
            _ => {}
        }
    }
    Ok(())
}

fn print_root_tree_summary_create(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    mode: Option<TreeMode>,
    id: Option<&ObjectId>,
    path: &[u8],
    relative_prefix: Option<&[u8]>,
) -> Result<()> {
    match (mode, id) {
        (Some(TreeMode::Tree), Some(id)) => {
            for child in tree_cache.read_tree(id)?.iter() {
                let child_path = root_tree_child_path(path, &child.name);
                print_root_tree_summary_create(
                    tree_cache,
                    Some(child.mode),
                    Some(&child.id),
                    &child_path,
                    relative_prefix,
                )?;
            }
        }
        (Some(mode), Some(_)) => {
            println!(
                " create mode {} {}",
                tree_mode_octal(mode),
                diff_display_path(path, relative_prefix)
            );
        }
        _ => {}
    }
    Ok(())
}

fn print_root_tree_summary_delete(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    mode: Option<TreeMode>,
    id: Option<&ObjectId>,
    path: &[u8],
    relative_prefix: Option<&[u8]>,
) -> Result<()> {
    match (mode, id) {
        (Some(TreeMode::Tree), Some(id)) => {
            for child in tree_cache.read_tree(id)?.iter() {
                let child_path = root_tree_child_path(path, &child.name);
                print_root_tree_summary_delete(
                    tree_cache,
                    Some(child.mode),
                    Some(&child.id),
                    &child_path,
                    relative_prefix,
                )?;
            }
        }
        (Some(mode), Some(_)) => {
            println!(
                " delete mode {} {}",
                tree_mode_octal(mode),
                diff_display_path(path, relative_prefix)
            );
        }
        _ => {}
    }
    Ok(())
}

fn root_tree_child_path(parent: &[u8], child: &[u8]) -> Vec<u8> {
    let mut path = Vec::with_capacity(parent.len() + 1 + child.len());
    path.extend_from_slice(parent);
    if !path.is_empty() {
        path.push(b'/');
    }
    path.extend_from_slice(child);
    path
}

pub(crate) fn apply_root_tree_diff_filter(
    entries: Vec<RootTreeDiffEntry>,
    diff_filter: DiffFilter,
) -> Vec<RootTreeDiffEntry> {
    if diff_filter.all_or_none {
        return if diff_filter.include_mask != 0
            && entries
                .iter()
                .any(|entry| diff_filter.include_mask & diff_filter_status_bit(entry.status) != 0)
        {
            entries
        } else {
            Vec::new()
        };
    }
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
            .map(|id| diff_raw_object_id_len(id, abbrev_len))
            .unwrap_or_else(|| diff_raw_zero_object_id_len(abbrev_len));
        let new_id = entry
            .new_id
            .as_ref()
            .map(|id| diff_raw_object_id_len(id, abbrev_len))
            .unwrap_or_else(|| diff_raw_zero_object_id_len(abbrev_len));
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

pub(crate) fn print_tree_raw_entries(
    store: &LooseObjectStore,
    old_tree: Option<&ObjectId>,
    new_tree: &ObjectId,
    pathspecs: &[Vec<u8>],
    abbrev_len: Option<usize>,
    relative_prefix: Option<&[u8]>,
    nul_terminated: bool,
) -> Result<()> {
    let _trace = phase_trace("show.raw.print_tree_raw_entries");
    let tree_cache = TreeObjectCache::new(store);
    if let Some(abbrev_len) = abbrev_len {
        let _trace = phase_trace("show.raw.print_tree_raw_entries.stream");
        return Ok(zmin_git_core::for_each_tree_diff(
            &tree_cache,
            old_tree,
            new_tree,
            |entry| {
                if !pathspecs.is_empty() && !pathspec_matches(&entry.path, pathspecs) {
                    return Ok(());
                }
                let old_mode = entry
                    .old_entry
                    .as_ref()
                    .map(|entry| index_mode_octal(entry.mode))
                    .unwrap_or("000000");
                let new_mode = entry
                    .new_entry
                    .as_ref()
                    .map(|entry| index_mode_octal(entry.mode))
                    .unwrap_or("000000");
                let old_id = entry
                    .old_entry
                    .as_ref()
                    .map(|entry| diff_raw_object_id_len(&entry.id, abbrev_len))
                    .unwrap_or_else(|| diff_raw_zero_object_id_len(abbrev_len));
                let new_id = entry
                    .new_entry
                    .as_ref()
                    .map(|entry| diff_raw_object_id_len(&entry.id, abbrev_len))
                    .unwrap_or_else(|| diff_raw_zero_object_id_len(abbrev_len));
                let path = diff_display_path(&entry.path, relative_prefix);
                if nul_terminated {
                    print!(
                        ":{old_mode} {new_mode} {old_id} {new_id} {}\0{path}\0",
                        entry.status.name_status()
                    );
                } else {
                    println!(
                        ":{old_mode} {new_mode} {old_id} {new_id} {}\t{path}",
                        entry.status.name_status()
                    );
                }
                Ok(())
            },
        )?);
    }
    let entries = {
        let _trace = phase_trace("show.raw.print_tree_raw_entries.collect");
        zmin_git_core::diff_trees(&tree_cache, old_tree, new_tree)?
            .into_iter()
            .filter(|entry| pathspecs.is_empty() || pathspec_matches(&entry.path, pathspecs))
            .collect::<Vec<_>>()
    };
    let abbrev_len = {
        let _trace = phase_trace("show.raw.print_tree_raw_entries.auto_abbrev");
        default_raw_abbrev_len_for_tree_entries(store, &entries)?
    };
    let _trace = phase_trace("show.raw.print_tree_raw_entries.render");
    for entry in entries {
        let old_mode = entry
            .old_entry
            .as_ref()
            .map(|entry| index_mode_octal(entry.mode))
            .unwrap_or("000000");
        let new_mode = entry
            .new_entry
            .as_ref()
            .map(|entry| index_mode_octal(entry.mode))
            .unwrap_or("000000");
        let old_id = entry
            .old_entry
            .as_ref()
            .map(|entry| diff_raw_object_id_len(&entry.id, abbrev_len))
            .unwrap_or_else(|| diff_raw_zero_object_id_len(abbrev_len));
        let new_id = entry
            .new_entry
            .as_ref()
            .map(|entry| diff_raw_object_id_len(&entry.id, abbrev_len))
            .unwrap_or_else(|| diff_raw_zero_object_id_len(abbrev_len));
        let path = diff_display_path(&entry.path, relative_prefix);
        if nul_terminated {
            print!(
                ":{old_mode} {new_mode} {old_id} {new_id} {}\0{path}\0",
                entry.status.name_status()
            );
        } else {
            println!(
                ":{old_mode} {new_mode} {old_id} {new_id} {}\t{path}",
                entry.status.name_status()
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
        || options.dirstat.is_some()
        || options.cumulative
        || options.dirstat_by_file.is_some()
}

pub(crate) struct DiffPairsBatch {
    pub(crate) old_index: GitIndex,
    pub(crate) new_index: GitIndex,
    pub(crate) entries: Vec<zmin_git_core::IndexDiffEntry>,
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
    entries: &mut Vec<zmin_git_core::IndexDiffEntry>,
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
    entries.push(zmin_git_core::IndexDiffEntry {
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
) -> Result<Vec<zmin_git_core::IndexDiffEntry>> {
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

pub(crate) fn filtered_diff_entries_pathspecs(
    old_index: &GitIndex,
    new_index: &GitIndex,
    pathspecs: &[Vec<u8>],
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
    find_copies_harder: bool,
) -> Result<Vec<zmin_git_core::IndexDiffEntry>> {
    Ok(diff_entries_for_indexes(
        old_index,
        new_index,
        detect_renames,
        detect_copies,
        find_copies_harder,
    )?
    .into_iter()
    .filter(|entry| diff_entry_matches_pathspec(entry, pathspecs))
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
    let word_diff = parse_word_diff_option(
        options
            .color_words
            .as_deref()
            .map(|_| "color")
            .or(options.word_diff.as_deref()),
    )?;
    let word_diff_regex = options
        .color_words
        .as_deref()
        .filter(|value| !value.is_empty())
        .or(options.word_diff_regex.as_deref());
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
        compact_summary: options.compact_summary,
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
        word_diff,
        word_diff_regex: word_diff_regex.map(str::to_owned),
        patch_abbrev_len,
        old_prefix,
        new_prefix,
        unified_context,
        inter_hunk_context,
        output_indicator_new,
        output_indicator_old,
        output_indicator_context,
        line_prefix: options.line_prefix.clone(),
        ignore_matching_lines,
        ignore_blank_lines: options.ignore_blank_lines,
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
    pub(crate) precomputed_entries: Option<Vec<zmin_git_core::IndexDiffEntry>>,
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
    let _trace = phase_trace("diff.parse_input.total");
    if !cached && args.len() == 2 {
        let old_arg = args[0].to_string_lossy();
        let new_arg = args[1].to_string_lossy();
        let _trace = phase_trace("diff.parse_input.two_arg_tree_probe");
        if let Ok(old_tree) = resolve_treeish(repo, store, &old_arg)
            && let Ok(new_tree) = resolve_treeish(repo, store, &new_arg)
        {
            if old_tree == new_tree {
                return Ok(DiffInput {
                    old_index: GitIndex::new(),
                    new_index: GitIndex::new(),
                    precomputed_entries: Some(Vec::new()),
                    new_side_from_index: true,
                    paths: Vec::new(),
                });
            }
            let tree_cache = TreeObjectCache::new(store);
            return Ok(DiffInput {
                old_index: tree_cache.read_tree_to_index(&old_tree)?,
                new_index: tree_cache.read_tree_to_index(&new_tree)?,
                precomputed_entries: None,
                new_side_from_index: true,
                paths: Vec::new(),
            });
        }
    }
    if !cached
        && let Some(input) = {
            let _trace = phase_trace("diff.parse_input.blob_pair_probe");
            parse_blob_pair_diff_input(repo, store, &args)?
        }
    {
        return Ok(input);
    }
    let (revs, paths) = {
        let _trace = phase_trace("diff.parse_input.split_revs_and_paths");
        split_diff_revs_and_paths(repo, store, args)?
    };
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative(repo, path))
        .collect::<Result<Vec<_>>>()?;
    let commit_cache = {
        let _trace = phase_trace("diff.parse_input.commit_cache");
        CommitObjectCache::new(store)
    };
    let tree_cache = {
        let _trace = phase_trace("diff.parse_input.tree_cache");
        TreeObjectCache::new(store)
    };
    match (cached, revs.as_slice()) {
        (true, []) => {
            let _trace = phase_trace("diff.parse_input.cached_head");
            let old_index = filter_index_matching_pathspecs(
                &read_head_index_with_caches(repo, &commit_cache, &tree_cache)?,
                &pathspecs,
            )?;
            let new_index = filter_index_matching_pathspecs(index, &pathspecs)?;
            Ok(DiffInput {
                old_index,
                new_index,
                precomputed_entries: None,
                new_side_from_index: true,
                paths,
            })
        }
        (true, [old]) => {
            let _trace = phase_trace("diff.parse_input.cached_treeish");
            let old_index = filter_index_matching_pathspecs(
                &read_treeish_index_cached(repo, store, &tree_cache, old)?,
                &pathspecs,
            )?;
            let new_index = filter_index_matching_pathspecs(index, &pathspecs)?;
            Ok(DiffInput {
                old_index,
                new_index,
                precomputed_entries: None,
                new_side_from_index: true,
                paths,
            })
        }
        (true, [_, _, ..]) => Err(CliError::Fatal {
            code: 129,
            message: "`diff --cached` accepts at most one commit".into(),
        }),
        (false, []) => {
            let _trace = phase_trace("diff.parse_input.worktree");
            let old_index = filter_index_matching_pathspecs(index, &pathspecs)?;
            let new_index = worktree_diff_index_snapshot(repo, &old_index)?;
            Ok(DiffInput {
                old_index,
                new_index,
                precomputed_entries: None,
                new_side_from_index: false,
                paths,
            })
        }
        (false, [old]) => {
            let _trace = phase_trace("diff.parse_input.worktree_treeish");
            let old_index = filter_index_matching_pathspecs(
                &read_treeish_index_cached(repo, store, &tree_cache, old)?,
                &pathspecs,
            )?;
            let new_index = worktree_diff_index_snapshot(
                repo,
                &filter_index_matching_pathspecs(index, &pathspecs)?,
            )?;
            Ok(DiffInput {
                old_index,
                new_index,
                precomputed_entries: None,
                new_side_from_index: false,
                paths,
            })
        }
        (false, [old, new]) => {
            let _trace = phase_trace("diff.parse_input.two_treeish");
            let old_tree = resolve_treeish(repo, store, old)?;
            let new_tree = resolve_treeish(repo, store, new)?;
            if old_tree == new_tree {
                return Ok(DiffInput {
                    old_index: GitIndex::new(),
                    new_index: GitIndex::new(),
                    precomputed_entries: Some(Vec::new()),
                    new_side_from_index: true,
                    paths,
                });
            }
            let old_index = filter_index_matching_pathspecs(
                &tree_cache.read_tree_to_index(&old_tree)?,
                &pathspecs,
            )?;
            let new_index = filter_index_matching_pathspecs(
                &tree_cache.read_tree_to_index(&new_tree)?,
                &pathspecs,
            )?;
            Ok(DiffInput {
                old_index,
                new_index,
                precomputed_entries: None,
                new_side_from_index: true,
                paths,
            })
        }
        (false, [_, _, ..]) => Err(CliError::Fatal {
            code: 129,
            message: "`diff` accepts at most two commits".into(),
        }),
    }
}

fn filter_index_matching_pathspecs(index: &GitIndex, pathspecs: &[Vec<u8>]) -> Result<GitIndex> {
    if pathspecs.is_empty() {
        return Ok(index.clone());
    }
    let entries = index
        .entries()
        .iter()
        .filter(|entry| {
            entry.stage == 0 && pathspec_matches(&entry.path, pathspecs)
                || entry.stage != 0 && pathspec_matches(&entry.path, pathspecs)
        })
        .cloned()
        .collect::<Vec<_>>();
    Ok(GitIndex::from_entries(entries)?)
}

struct BlobDiffSide {
    entry: IndexEntry,
}

fn parse_blob_pair_diff_input(
    repo: &GitRepo,
    store: &LooseObjectStore,
    args: &[PathBuf],
) -> Result<Option<DiffInput>> {
    if args.len() != 2 {
        return Ok(None);
    }
    let old_arg = args[0].to_string_lossy();
    let new_arg = args[1].to_string_lossy();
    let Some(old_side) = resolve_blob_diff_side(repo, store, &old_arg)? else {
        if diff_args_resolve_to_mixed_object_pair(repo, store, &old_arg, &new_arg)? {
            return Err(diff_usage_error());
        }
        return Ok(None);
    };
    let Some(new_side) = resolve_blob_diff_side(repo, store, &new_arg)? else {
        if diff_args_resolve_to_mixed_object_pair(repo, store, &old_arg, &new_arg)? {
            return Err(diff_usage_error());
        }
        return Ok(None);
    };
    let old_index = GitIndex::from_entries(vec![old_side.entry.clone()])?;
    let new_index = GitIndex::from_entries(vec![new_side.entry.clone()])?;
    let precomputed_entries =
        if old_side.entry.id == new_side.entry.id && old_side.entry.mode == new_side.entry.mode {
            Vec::new()
        } else {
            vec![zmin_git_core::IndexDiffEntry {
                status: IndexDiffStatus::Modified,
                path: new_side.entry.path.clone(),
                old_path: Some(old_side.entry.path.clone()),
                similarity: None,
            }]
        };
    Ok(Some(DiffInput {
        old_index,
        new_index,
        precomputed_entries: Some(precomputed_entries),
        new_side_from_index: true,
        paths: Vec::new(),
    }))
}

fn diff_args_resolve_to_mixed_object_pair(
    repo: &GitRepo,
    store: &LooseObjectStore,
    old: &str,
    new: &str,
) -> Result<bool> {
    if resolve_treeish(repo, store, old).is_ok() && resolve_treeish(repo, store, new).is_ok() {
        return Ok(false);
    }
    Ok(diff_operand_object_kind(repo, store, old)?.is_some()
        && diff_operand_object_kind(repo, store, new)?.is_some())
}

fn diff_operand_object_kind(
    repo: &GitRepo,
    store: &LooseObjectStore,
    objectish: &str,
) -> Result<Option<GitObjectKind>> {
    let resolved = match resolve_objectish_with_mode(repo, objectish) {
        Ok(resolved) => resolved,
        Err(_) => return Ok(None),
    };
    Ok(Some(store.read_object(&resolved.id)?.kind))
}

pub(crate) fn diff_usage_error() -> CliError {
    CliError::Stderr {
        code: 129,
        text: concat!(
            "usage: git diff [<options>] [<commit>] [--] [<path>...]\n",
            "   or: git diff [<options>] --cached [--merge-base] [<commit>] [--] [<path>...]\n",
            "   or: git diff [<options>] [--merge-base] <commit> [<commit>...] <commit> [--] [<path>...]\n",
            "   or: git diff [<options>] <commit>...<commit> [--] [<path>...]\n",
            "   or: git diff [<options>] <blob> <blob>\n",
            "   or: git diff [<options>] --no-index [--] <path> <path>\n",
            "\n",
            "common diff options:\n",
            "  -z            output diff-raw with lines terminated with NUL.\n",
            "  -p            output patch format.\n",
            "  -u            synonym for -p.\n",
            "  --patch-with-raw\n",
            "                output both a patch and the diff-raw format.\n",
            "  --stat        show diffstat instead of patch.\n",
            "  --numstat     show numeric diffstat instead of patch.\n",
            "  --patch-with-stat\n",
            "                output a patch and prepend its diffstat.\n",
            "  --name-only   show only names of changed files.\n",
            "  --name-status show names and status of changed files.\n",
            "  --full-index  show full object name on index lines.\n",
            "  --abbrev=<n>  abbreviate object names in diff-tree header and diff-raw.\n",
            "  -R            swap input file pairs.\n",
            "  -B            detect complete rewrites.\n",
            "  -M            detect renames.\n",
            "  -C            detect copies.\n",
            "  --find-copies-harder\n",
            "                try unchanged files as candidate for copy detection.\n",
            "  -l<n>         limit rename attempts up to <n> paths.\n",
            "  -O<file>      reorder diffs according to the <file>.\n",
            "  -S<string>    find filepair whose only one side contains the string.\n",
            "  --pickaxe-all\n",
            "                show all files diff when -S is used and hit is found.\n",
            "  -a  --text    treat all files as text.\n",
            "\n",
        )
        .into(),
    }
}

fn resolve_blob_diff_side(
    repo: &GitRepo,
    store: &LooseObjectStore,
    objectish: &str,
) -> Result<Option<BlobDiffSide>> {
    let resolved = match resolve_objectish_with_mode(repo, objectish) {
        Ok(resolved) => resolved,
        Err(_) => return Ok(None),
    };
    let object = store.read_object(&resolved.id)?;
    if object.kind != GitObjectKind::Blob {
        return Ok(None);
    }
    let mode = resolved
        .mode
        .as_deref()
        .map(parse_index_mode)
        .transpose()?
        .unwrap_or(IndexMode::File);
    let path =
        objectish_path_component(objectish)?.unwrap_or_else(|| objectish.as_bytes().to_vec());
    let size = u32::try_from(object.content.len()).unwrap_or(u32::MAX);
    Ok(Some(BlobDiffSide {
        entry: IndexEntry::new(path, resolved.id, mode, size)?,
    }))
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
        if revs.is_empty()
            && let Some((old, new)) = arg.split_once("..")
            && !old.is_empty()
            && !new.is_empty()
            && !new.contains("..")
            && resolve_treeish(repo, store, old).is_ok()
            && resolve_treeish(repo, store, new).is_ok()
        {
            revs.push(old.to_owned());
            revs.push(new.to_owned());
            path_start = idx + 1;
            continue;
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
        .map(|value| {
            if value.is_empty() {
                Ok(7)
            } else {
                parse_stash_show_abbrev(value)
            }
        })
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
    entries: Vec<zmin_git_core::IndexDiffEntry>,
    prefix: Option<&[u8]>,
) -> Vec<zmin_git_core::IndexDiffEntry> {
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
                message: format!("invalid regex given to -I: '{pattern}': {error}"),
            })
        })
        .collect()
}

#[derive(Clone, Copy, Default)]
pub(crate) struct DiffFilter {
    include_mask: u16,
    exclude_mask: u16,
    all_or_none: bool,
}

impl DiffFilter {
    pub(crate) fn excludes_unmerged(self) -> bool {
        self.exclude_mask & diff_filter_bit(b'u').expect("known diff filter") != 0
    }
}

pub(crate) fn parse_diff_filter(value: &str) -> Result<DiffFilter> {
    let mut filter = DiffFilter::default();
    for byte in value.bytes() {
        if byte == b'*' {
            filter.all_or_none = true;
            continue;
        }
        let bit = diff_filter_bit(byte.to_ascii_lowercase()).ok_or_else(|| CliError::Stderr {
            code: 129,
            text: format!(
                "error: unknown change class '{}' in --diff-filter={value}\n",
                byte as char
            ),
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
    entries: Vec<zmin_git_core::IndexDiffEntry>,
    filter: DiffFilter,
) -> Vec<zmin_git_core::IndexDiffEntry> {
    if filter.include_mask == 0 && filter.exclude_mask == 0 {
        return entries;
    }
    if filter.all_or_none {
        return if filter.include_mask != 0
            && entries
                .iter()
                .any(|entry| filter.include_mask & diff_filter_status_bit(entry.status) != 0)
        {
            entries
        } else {
            Vec::new()
        };
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

fn format_patch_diff_entries(
    context: &FormatPatchContext<'_>,
    old_index: &GitIndex,
    new_index: &GitIndex,
) -> Result<Vec<zmin_git_core::IndexDiffEntry>> {
    let entries = diff_entries_for_indexes(
        old_index,
        new_index,
        context.rename_threshold,
        context.copy_threshold,
        context.find_copies_harder,
    )?;
    if context.rename_threshold.is_some_and(|value| value < 100)
        || context.copy_threshold.is_some_and(|value| value < 100)
    {
        let diff_context = DiffIndexContext {
            repo: context.repo,
            store: context.store,
            old_index,
            new_index,
            old_source: DiffSideSource::Index,
            new_source: DiffSideSource::Index,
        };
        apply_similarity_detection(
            &diff_context,
            entries,
            SimilarityDetectionOptions {
                rename_threshold: context.rename_threshold,
                copy_threshold: context.copy_threshold,
                find_copies_harder: context.find_copies_harder,
            },
        )
    } else {
        Ok(entries)
    }
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
    let mut old_index = old_tree
        .map(|tree| tree_cache.read_tree_to_index(tree))
        .transpose()?
        .unwrap_or_else(GitIndex::new);
    let mut new_index = tree_cache.read_tree_to_index(new_tree)?;
    let mut entries = format_patch_diff_entries(context, &old_index, &new_index)?;
    if !context.pathspecs.is_empty() {
        entries.retain(|entry| diff_entry_matches_pathspec(entry, context.pathspecs));
    }
    entries = apply_diff_order_file(entries, context.order_file)?;
    entries = apply_diff_skip_rotate(entries, context.skip_to, context.rotate_to);
    if context.reverse {
        reverse_index_diff_entries(&mut entries);
        std::mem::swap(&mut old_index, &mut new_index);
    }
    {
        let _trace = phase_trace("format_patch.write_header");
        write_format_patch_header(out, context, id, commit, number)?;
    }
    if context.attach || context.inline {
        let _trace = phase_trace("format_patch.write_attach_body");
        write_format_patch_attach_body(
            out, context, entry, &old_index, &new_index, &entries, blob_cache,
        )?;
        return Ok(());
    }
    {
        let _trace = phase_trace("format_patch.write_prelude");
        write_format_patch_prelude(out, context, id, &old_index, &new_index, &entries)?;
    }
    {
        let _trace = phase_trace("format_patch.write_tree_diff");
        write_format_patch_entries(out, context, &old_index, &new_index, &entries, blob_cache)?;
    }
    if !context.attach && !context.inline {
        if let Some(base_information) = context.base_information {
            if number == 1 && !context.cover_letter_base_emitted(number) {
                write_format_patch_base_information(out, base_information, true)?;
            }
        }
        if let Some(appendix) = context.appendix.filter(|_| !context.cover_letter) {
            write_format_patch_appendix(out, appendix, true)?;
        }
    }
    write_format_patch_footer(out, context.signature)?;
    Ok(())
}

fn write_format_patch_attach_body<W: Write>(
    out: &mut W,
    context: &FormatPatchContext<'_>,
    entry: FormatPatchEntry<'_>,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
    blob_cache: &mut FormatPatchBlobCache<'_>,
) -> Result<()> {
    let mime_boundary = format_patch_mime_boundary(context);
    let subject = commit_subject_view(&entry.commit.message);
    let filename = if context.numbered_files {
        entry.number.to_string()
    } else {
        format_patch_output_filename(entry.number, &subject, context)
    };
    writeln!(out, "This is a multi-part message in MIME format.")?;
    writeln!(out, "--------------{mime_boundary}")?;
    writeln!(out, "Content-Type: text/plain; charset=UTF-8; format=fixed")?;
    writeln!(out, "Content-Transfer-Encoding: 8bit")?;
    writeln!(out)?;
    if !commit_message_body_view(&entry.commit.message).is_empty() {
        writeln!(out)?;
        write_commit_message_body(out, &entry.commit.message, context.mboxrd)?;
    }
    write_format_patch_prelude(out, context, entry.id, old_index, new_index, entries)?;
    writeln!(out)?;
    writeln!(out, "--------------{mime_boundary}")?;
    writeln!(out, "Content-Type: text/x-patch; name=\"{filename}\"")?;
    writeln!(out, "Content-Transfer-Encoding: 8bit")?;
    let disposition = if context.inline {
        "inline"
    } else {
        "attachment"
    };
    writeln!(
        out,
        "Content-Disposition: {disposition}; filename=\"{filename}\""
    )?;
    writeln!(out)?;
    write_format_patch_entries(out, context, old_index, new_index, entries, blob_cache)?;
    if let Some(base_information) = context.base_information
        && entry.number == 1
        && !context.cover_letter_base_emitted(entry.number)
    {
        write_format_patch_base_information(out, base_information, true)?;
    }
    if let Some(appendix) = context.appendix.filter(|_| !context.cover_letter) {
        write_format_patch_appendix(out, appendix, true)?;
    }
    writeln!(out)?;
    writeln!(out, "--------------{mime_boundary}--")?;
    writeln!(out)?;
    writeln!(out)?;
    Ok(())
}

fn write_format_patch_prelude<W: Write>(
    out: &mut W,
    context: &FormatPatchContext<'_>,
    id: &ObjectId,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
) -> Result<()> {
    let notes = context.notes_for(id);
    if entries.is_empty() && notes.is_none() {
        return Ok(());
    }
    let diff_context = DiffIndexContext {
        repo: context.repo,
        store: context.store,
        old_index,
        new_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    let stat_options = DiffStatOptions {
        whitespace_mode: DiffWhitespaceMode::None,
        relative_prefix: context.relative_prefix.as_deref(),
        ignore_matching_lines: &[],
        ignore_blank_lines: false,
        compact_summary: false,
        color: context.word_diff == WordDiffMode::Color,
    };
    match context.prelude_mode {
        FormatPatchPreludeMode::Diffstat => {
            writeln!(out, "---")?;
            write_format_patch_note_blocks(out, notes, true)?;
            write_stat_entries(out, &diff_context, entries, stat_options)?;
            write_summary_entries(out, old_index, new_index, entries, None)?;
            write_format_patch_prelude_separator(out, context.nul_terminated)?;
        }
        FormatPatchPreludeMode::None => {
            write_format_patch_note_blocks(out, notes, false)?;
        }
        FormatPatchPreludeMode::Raw => {
            write_format_patch_note_blocks(out, notes, false)?;
            write_raw_entries_to(out, &diff_context, entries, context.abbrev_len, None)?;
            write_format_patch_prelude_separator(out, context.nul_terminated)?;
        }
        FormatPatchPreludeMode::Numstat => {
            write_format_patch_note_blocks(out, notes, false)?;
            write_numstat_entries_to(
                out,
                &diff_context,
                entries,
                NumstatOptions {
                    stat: stat_options,
                    nul_terminated: context.nul_terminated,
                },
            )?;
            write_format_patch_prelude_separator(out, context.nul_terminated)?;
        }
        FormatPatchPreludeMode::Dirstat => {
            write_format_patch_note_blocks(out, notes, false)?;
            print_dirstat_entries(out, &diff_context, entries, stat_options, false, false)?;
        }
        FormatPatchPreludeMode::DirstatByFile => {
            write_format_patch_note_blocks(out, notes, false)?;
            print_dirstat_entries(out, &diff_context, entries, stat_options, true, false)?;
        }
        FormatPatchPreludeMode::Shortstat => {
            write_format_patch_note_blocks(out, notes, false)?;
            write_shortstat_entries_to(out, &diff_context, entries, stat_options)?;
            write_format_patch_prelude_separator(out, context.nul_terminated)?;
        }
        FormatPatchPreludeMode::Summary => {
            write_format_patch_note_blocks(out, notes, false)?;
            let mut summary = Vec::new();
            write_summary_entries(&mut summary, old_index, new_index, entries, None)?;
            if !summary.is_empty() {
                out.write_all(&summary)?;
                write_format_patch_prelude_separator(out, context.nul_terminated)?;
            }
        }
    }
    Ok(())
}

fn write_format_patch_note_blocks<W: Write>(
    out: &mut W,
    notes: Option<&[FormatPatchNoteBlock]>,
    separator_written: bool,
) -> Result<()> {
    let Some(notes) = notes else {
        return Ok(());
    };
    if separator_written {
        writeln!(out)?;
    } else {
        writeln!(out, "---")?;
        writeln!(out)?;
    }
    for (index, note) in notes.iter().enumerate() {
        if index > 0 {
            writeln!(out)?;
        }
        if let Some(label) = &note.label {
            writeln!(out, "Notes ({label}):")?;
        } else {
            writeln!(out, "Notes:")?;
        }
        for line in note.text.trim_end_matches('\n').split('\n') {
            if line.is_empty() {
                writeln!(out)?;
            } else {
                writeln!(out, "    {line}")?;
            }
        }
    }
    if !notes.is_empty() {
        writeln!(out)?;
    }
    Ok(())
}

fn write_format_patch_prelude_separator<W: Write>(out: &mut W, nul_terminated: bool) -> Result<()> {
    if nul_terminated {
        out.write_all(b"\0")?;
    } else {
        writeln!(out)?;
    }
    Ok(())
}

fn write_format_patch_entries<W: Write>(
    out: &mut W,
    context: &FormatPatchContext<'_>,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
    _blob_cache: &mut FormatPatchBlobCache<'_>,
) -> Result<()> {
    validate_word_diff_regex(context.word_diff_regex)?;
    let (mut old_prefix, mut new_prefix) = diff_prefixes(context.no_prefix, false, None, None);
    if context.reverse {
        std::mem::swap(&mut old_prefix, &mut new_prefix);
    }
    let mut format = PatchFormatOptions::cached()
        .with_abbrev_len(Some(context.patch_abbrev_len))
        .with_prefixes(old_prefix, new_prefix)
        .with_context(context.unified_context, 0)
        .with_binary(true)
        .with_color_mode(if context.word_diff == WordDiffMode::Color {
            DiffColorMode::Always
        } else {
            DiffColorMode::Never
        })
        .with_submodule_format(context.submodule_format);
    format.word_diff = context.word_diff;
    format.word_diff_regex = context.word_diff_regex.map(str::to_owned);
    format.relative_prefix = context.relative_prefix.clone();
    write_patch_entries(
        out,
        context.repo,
        context.store,
        old_index,
        new_index,
        entries,
        format,
    )
}

pub(crate) fn write_format_patch_cover_letter<W: Write, S: GitObjectStore + ?Sized>(
    out: &mut W,
    context: &FormatPatchContext<'_>,
    id: &ObjectId,
    cover_signature: &[u8],
    commits: &[CollectedCommit],
    tree_cache: &TreeObjectCache<'_, S>,
    old_tree: Option<&ObjectId>,
    new_tree: &ObjectId,
) -> Result<()> {
    let mut old_index = old_tree
        .map(|tree| tree_cache.read_tree_to_index(tree))
        .transpose()?
        .unwrap_or_else(GitIndex::new);
    let mut new_index = tree_cache.read_tree_to_index(new_tree)?;
    let mut entries = format_patch_diff_entries(context, &old_index, &new_index)?;
    if !context.pathspecs.is_empty() {
        entries.retain(|entry| diff_entry_matches_pathspec(entry, context.pathspecs));
    }
    entries = apply_diff_order_file(entries, context.order_file)?;
    entries = apply_diff_skip_rotate(entries, context.skip_to, context.rotate_to);
    if context.reverse {
        reverse_index_diff_entries(&mut entries);
        std::mem::swap(&mut old_index, &mut new_index);
    }
    write!(out, "From ")?;
    id.write_hex_io(out)?;
    writeln!(out, " Mon Sep 17 00:00:00 2001")?;
    write_format_patch_thread_headers(out, context, None)?;
    let cover_from = context
        .sender_override
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format_patch_author_header_value(cover_signature, context.encode_email_headers)
        });
    write_format_patch_folded_header(out, "From", &cover_from)?;
    writeln!(out, "Date: {}", signature_mail_date(cover_signature)?)?;
    let total = context.total + context.number_offset;
    let cover_subject_prefix = format_patch_subject_prefix_label(context.subject_prefix);
    let cover_subject = context.cover_subject.unwrap_or("*** SUBJECT HERE ***");
    if (context.total > 1 && !context.no_numbered) || context.numbered {
        if cover_subject_prefix.is_empty() {
            let prefix = format!("[0/{total}] ");
            write_format_patch_subject_header(
                out,
                &prefix,
                cover_subject,
                context.encode_email_headers,
            )?;
        } else {
            let prefix = format!("[{cover_subject_prefix} 0/{total}] ");
            write_format_patch_subject_header(
                out,
                &prefix,
                cover_subject,
                context.encode_email_headers,
            )?;
        }
    } else {
        let prefix = format!("[{cover_subject_prefix}] ");
        write_format_patch_subject_header(
            out,
            &prefix,
            cover_subject,
            context.encode_email_headers,
        )?;
    }
    if context.include_mime_headers && !context.attach && !context.inline {
        writeln!(out, "MIME-Version: 1.0")?;
        writeln!(out, "Content-Type: text/plain; charset=UTF-8")?;
        writeln!(out, "Content-Transfer-Encoding: 8bit")?;
    }
    writeln!(out)?;
    writeln!(
        out,
        "{}",
        context.cover_blurb.unwrap_or("*** BLURB HERE ***")
    )?;
    writeln!(out)?;
    write_format_patch_cover_commit_list(out, context, commits)?;
    writeln!(out)?;
    write_format_patch_cover_prelude(out, context, &old_index, &new_index, &entries)?;
    if let Some(base_information) = context.base_information {
        write_format_patch_base_information(out, base_information, false)?;
    }
    if let Some(appendix) = context.appendix {
        write_format_patch_appendix(out, appendix, true)?;
    }
    write_format_patch_footer(out, context.signature)?;
    Ok(())
}

fn write_format_patch_thread_headers<W: Write>(
    out: &mut W,
    context: &FormatPatchContext<'_>,
    number: Option<usize>,
) -> Result<()> {
    let Some(style) = context.thread else {
        if let Some(in_reply_to) = context.in_reply_to {
            let in_reply_to = normalize_message_id_header(in_reply_to);
            writeln!(out, "In-Reply-To: {in_reply_to}")?;
            writeln!(out, "References: {in_reply_to}")?;
        }
        return Ok(());
    };
    let Some(message_id) = format_patch_thread_message_id(context, number) else {
        return Ok(());
    };
    writeln!(out, "Message-ID: {message_id}")?;

    let mut references = Vec::new();
    let in_reply_to = if let Some(number) = number {
        format_patch_thread_parent(context, style, number, &mut references)
    } else {
        context.in_reply_to.map(|id| {
            let id = normalize_message_id_header(id);
            references.push(id.clone());
            id
        })
    };
    if let Some(in_reply_to) = in_reply_to {
        writeln!(out, "In-Reply-To: {in_reply_to}")?;
    }
    write_format_patch_references_header(out, &references)
}

fn format_patch_thread_message_id(
    context: &FormatPatchContext<'_>,
    number: Option<usize>,
) -> Option<String> {
    let timestamp = context.message_id_timestamp?;
    let sequence = match number {
        None => 0,
        Some(number) if context.cover_letter => number,
        Some(number) => number.saturating_sub(1),
    };
    Some(format!("<{sequence}.{timestamp}.git.zmin@example.test>"))
}

fn format_patch_thread_parent(
    context: &FormatPatchContext<'_>,
    style: &str,
    number: usize,
    references: &mut Vec<String>,
) -> Option<String> {
    let external = context.in_reply_to.map(normalize_message_id_header);
    if number == 1 && !context.cover_letter {
        if let Some(external) = external {
            references.push(external.clone());
            return Some(external);
        }
        return None;
    }

    if style == "deep" {
        let parent = if number == 1 {
            format_patch_thread_message_id(context, None)?
        } else {
            format_patch_thread_message_id(context, Some(number - 1))?
        };
        if let Some(external) = external {
            references.push(external);
        }
        let start = if context.cover_letter { 0 } else { 1 };
        for ancestor in start..number {
            let ancestor_number = if context.cover_letter && ancestor == 0 {
                None
            } else {
                Some(ancestor)
            };
            if let Some(message_id) = format_patch_thread_message_id(context, ancestor_number) {
                references.push(message_id);
            }
        }
        return Some(parent);
    }

    let parent = if !context.cover_letter && external.is_some() {
        external.clone()?
    } else if context.cover_letter {
        format_patch_thread_message_id(context, None)?
    } else {
        format_patch_thread_message_id(context, Some(1))?
    };
    if let Some(external) = external {
        references.push(external.clone());
        if !context.cover_letter {
            return Some(external);
        }
    }
    references.push(parent.clone());
    Some(parent)
}

fn write_format_patch_references_header<W: Write>(
    out: &mut W,
    references: &[String],
) -> Result<()> {
    let Some((first, rest)) = references.split_first() else {
        return Ok(());
    };
    writeln!(out, "References: {first}")?;
    for reference in rest {
        writeln!(out, "\t{reference}")?;
    }
    Ok(())
}

fn write_format_patch_cover_prelude<W: Write>(
    out: &mut W,
    context: &FormatPatchContext<'_>,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
) -> Result<()> {
    if context.prelude_mode != FormatPatchPreludeMode::Diffstat {
        let zero_hex = "0".repeat(context.store.algorithm().digest_len() * 2);
        let zero_id = ObjectId::from_hex(context.store.algorithm(), &zero_hex)?;
        return write_format_patch_prelude(out, context, &zero_id, old_index, new_index, entries);
    }
    if entries.is_empty() {
        return Ok(());
    }
    let diff_context = DiffIndexContext {
        repo: context.repo,
        store: context.store,
        old_index,
        new_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    let stat_options = DiffStatOptions {
        whitespace_mode: DiffWhitespaceMode::None,
        relative_prefix: context.relative_prefix.as_deref(),
        ignore_matching_lines: &[],
        ignore_blank_lines: false,
        compact_summary: false,
        color: context.word_diff == WordDiffMode::Color,
    };
    write_stat_entries(out, &diff_context, entries, stat_options)?;
    write_summary_entries(out, old_index, new_index, entries, None)?;
    writeln!(out)?;
    Ok(())
}

fn write_format_patch_cover_author_summary<W: Write>(
    out: &mut W,
    commits: &[CollectedCommit],
) -> Result<()> {
    let mut groups = Vec::<(String, Vec<Cow<'_, str>>)>::new();
    for entry in commits {
        let name = signature_name(&entry.commit.author);
        let subject = commit_subject_view(&entry.commit.message);
        if let Some((_, subjects)) = groups
            .iter_mut()
            .find(|(group_name, _)| *group_name == name)
        {
            subjects.push(subject);
        } else {
            groups.push((name, vec![subject]));
        }
    }
    for (name, subjects) in groups {
        writeln!(out, "{name} ({}):", subjects.len())?;
        for subject in subjects {
            write_wrapped_format_patch_cover_subject(out, subject.as_ref())?;
        }
    }
    Ok(())
}

fn write_wrapped_format_patch_cover_subject<W: Write>(out: &mut W, subject: &str) -> Result<()> {
    const WIDTH: usize = 72;
    const FIRST_INDENT: &str = "  ";
    const CONTINUATION_INDENT: &str = "    ";
    let words = format_patch_fold_tokens(subject);
    if words.is_empty() {
        writeln!(out, "{FIRST_INDENT}")?;
        return Ok(());
    }
    let mut current_indent = FIRST_INDENT;
    let mut current_len = current_indent.len();
    write!(out, "{current_indent}")?;
    let mut first_word = true;
    for word in words {
        let additional = if first_word {
            word.len()
        } else {
            1 + word.len()
        };
        if !first_word && current_len + additional > WIDTH {
            writeln!(out)?;
            current_indent = CONTINUATION_INDENT;
            write!(out, "{current_indent}")?;
            current_len = current_indent.len();
            first_word = true;
        }
        if !first_word {
            write!(out, " ")?;
            current_len += 1;
        }
        write!(out, "{word}")?;
        current_len += word.len();
        first_word = false;
    }
    writeln!(out)?;
    Ok(())
}

fn write_format_patch_cover_commit_list<W: Write>(
    out: &mut W,
    context: &FormatPatchContext<'_>,
    commits: &[CollectedCommit],
) -> Result<()> {
    match context.commit_list_format {
        Some("modern") => {
            for (index, entry) in commits.iter().enumerate() {
                let subject = commit_subject_view(&entry.commit.message);
                writeln!(out, "[{}/{}] {}", index + 1, commits.len(), subject)?;
            }
            Ok(())
        }
        Some("shortlog") | None => write_format_patch_cover_author_summary(out, commits),
        Some(value) => {
            let template = value.strip_prefix("log:").unwrap_or(value);
            for (index, entry) in commits.iter().enumerate() {
                let subject = commit_subject_view(&entry.commit.message);
                let line = template
                    .replace("%(count)", &(index + 1).to_string())
                    .replace("%(total)", &commits.len().to_string())
                    .replace("%s", subject.as_ref())
                    .replace("%an", &signature_name(&entry.commit.author));
                writeln!(out, "{line}")?;
            }
            Ok(())
        }
    }
}

fn write_format_patch_header<W: Write>(
    out: &mut W,
    context: &FormatPatchContext<'_>,
    id: &ObjectId,
    commit: &CommitObject,
    number: usize,
) -> Result<()> {
    let mime_boundary = format_patch_mime_boundary(context);
    let FormatPatchContext {
        total,
        no_numbered,
        numbered,
        subject_prefix,
        ..
    } = context;
    let number = number + context.number_offset;
    let total = total + context.number_offset;
    let subject = format_patch_subject(&commit.message, context.keep_subject);
    let subject_prefix = format_patch_subject_prefix_label(subject_prefix);
    if context.zero_commit {
        writeln!(
            out,
            "From 0000000000000000000000000000000000000000 Mon Sep 17 00:00:00 2001"
        )?;
    } else {
        write!(out, "From ")?;
        id.write_hex_io(out)?;
        writeln!(out, " Mon Sep 17 00:00:00 2001")?;
    }
    write_format_patch_thread_headers(out, context, Some(number))?;
    let sender_header = context
        .sender_override
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format_patch_author_header_value(&commit.author, context.encode_email_headers)
        });
    let author_header = format_patch_author_header_value(&commit.author, false);
    write_format_patch_folded_header(out, "From", &sender_header)?;
    writeln!(out, "Date: {}", signature_mail_date(&commit.author)?)?;
    if context.keep_subject {
        write_format_patch_folded_header(out, "Subject", &subject)?;
    } else if (total > 1 && !*no_numbered) || *numbered {
        if subject_prefix.is_empty() {
            let prefix = format!("[{number}/{total}] ");
            write_format_patch_subject_header(
                out,
                &prefix,
                &subject,
                context.encode_email_headers,
            )?;
        } else {
            let prefix = format!("[{subject_prefix} {number}/{total}] ");
            write_format_patch_subject_header(
                out,
                &prefix,
                &subject,
                context.encode_email_headers,
            )?;
        }
    } else {
        let prefix = format!("[{subject_prefix}] ");
        write_format_patch_subject_header(out, &prefix, &subject, context.encode_email_headers)?;
    }
    for header in context.extra_headers {
        writeln!(out, "{header}")?;
    }
    if context.attach || context.inline {
        writeln!(out, "MIME-Version: 1.0")?;
        writeln!(
            out,
            "Content-Type: multipart/mixed; boundary=\"------------{mime_boundary}\""
        )?;
        writeln!(out)?;
        return Ok(());
    }
    let body_from_line = if sender_header != author_header || context.body_from_override {
        Some(format!("From: {author_header}"))
    } else {
        None
    };
    if context.include_mime_headers
        || (context.encode_email_headers && !subject.is_ascii())
        || body_from_line
            .as_deref()
            .is_some_and(|line| !line.is_ascii())
    {
        writeln!(out, "MIME-Version: 1.0")?;
        writeln!(out, "Content-Type: text/plain; charset=UTF-8")?;
        writeln!(out, "Content-Transfer-Encoding: 8bit")?;
    }
    writeln!(out)?;
    write_format_patch_message_body(
        out,
        context.repo,
        &commit.message,
        body_from_line.as_deref(),
        context.signoff_line,
        context.mboxrd,
    )?;
    if context.prelude_mode != FormatPatchPreludeMode::Diffstat {
        writeln!(out)?;
    }
    Ok(())
}

fn format_patch_mime_boundary(context: &FormatPatchContext<'_>) -> String {
    if let Some(boundary) = context.mime_boundary.filter(|value| !value.is_empty()) {
        return boundary.to_owned();
    }
    crate::runtime::git_compatible_version_line()
        .strip_prefix("git version ")
        .unwrap_or(crate::runtime::GIT_COMPAT_VERSION)
        .to_owned()
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

fn write_format_patch_message_body<W: Write>(
    out: &mut W,
    repo: &GitRepo,
    message: &[u8],
    body_from_line: Option<&str>,
    signoff_line: Option<&str>,
    mboxrd: bool,
) -> Result<()> {
    if let Some(body_from_line) = body_from_line {
        writeln!(out, "{body_from_line}")?;
        writeln!(out)?;
    }
    write_commit_message_body(out, message, mboxrd)?;
    if let Some(signoff_line) = signoff_line {
        match format_patch_signoff_mode(repo, message, signoff_line)? {
            FormatPatchSignoffMode::Skip => return Ok(()),
            FormatPatchSignoffMode::AppendWithBlankLine => {
                writeln!(out)?;
            }
            FormatPatchSignoffMode::AppendAdjacent => {}
        }
        writeln!(out, "{signoff_line}")?;
    }
    Ok(())
}

enum FormatPatchSignoffMode {
    Skip,
    AppendAdjacent,
    AppendWithBlankLine,
}

fn format_patch_signoff_mode(
    repo: &GitRepo,
    message: &[u8],
    signoff_line: &str,
) -> Result<FormatPatchSignoffMode> {
    let body = commit_message_body_view(message);
    if body.is_empty() {
        return Ok(FormatPatchSignoffMode::AppendAdjacent);
    }
    let Some(signoff_suffix) = signoff_line.strip_prefix("Signed-off-by:") else {
        return Ok(FormatPatchSignoffMode::AppendWithBlankLine);
    };
    let configured_footer_keys = format_patch_trailer_footer_keys(repo)?;
    let lines = format_patch_split_body_lines(body);
    let footer_start = format_patch_footer_start(&lines, &configured_footer_keys);
    if let Some(start) = footer_start {
        if format_patch_footer_contains_signoff(&lines[start..], signoff_suffix) {
            return Ok(FormatPatchSignoffMode::Skip);
        }
        return Ok(FormatPatchSignoffMode::AppendAdjacent);
    }
    Ok(FormatPatchSignoffMode::AppendWithBlankLine)
}

fn format_patch_trailer_footer_keys(repo: &GitRepo) -> Result<HashSet<String>> {
    let mut keys = HashSet::new();
    for entry in read_config_entries(repo).map_err(CliError::Io)? {
        if entry.section != "trailer" || entry.subsection.is_empty() {
            continue;
        }
        let Some(value) = (match entry.key.as_str() {
            "key" => Some(entry.value.trim()),
            "ifexists" | "ifmissing" | "where" | "cmd" | "command" => {
                Some(entry.subsection.as_str())
            }
            _ => None,
        }) else {
            continue;
        };
        if !value.is_empty() {
            keys.insert(value.to_ascii_lowercase());
        }
    }
    Ok(keys)
}

fn format_patch_split_body_lines(body: &[u8]) -> Vec<&[u8]> {
    let body = body.strip_suffix(b"\n").unwrap_or(body);
    body.split(|byte| *byte == b'\n').collect()
}

fn format_patch_footer_start(
    lines: &[&[u8]],
    configured_footer_keys: &HashSet<String>,
) -> Option<usize> {
    let mut paragraph_start = lines.len();
    while paragraph_start > 0 && !format_patch_line_is_blank(lines[paragraph_start - 1]) {
        paragraph_start -= 1;
    }
    let paragraph = &lines[paragraph_start..];
    let trailer_positions = paragraph
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            format_patch_line_footer_key(line, configured_footer_keys).map(|_| index)
        })
        .collect::<Vec<_>>();
    let mut start = *trailer_positions.last()?;
    while start > 0 {
        let previous = start - 1;
        if format_patch_line_footer_key(paragraph[previous], configured_footer_keys).is_some() {
            start = previous;
            continue;
        }
        if trailer_positions
            .iter()
            .copied()
            .any(|index| index < previous)
        {
            start = previous;
            continue;
        }
        break;
    }
    Some(paragraph_start + start)
}

fn format_patch_line_footer_key(
    line: &[u8],
    configured_footer_keys: &HashSet<String>,
) -> Option<String> {
    let line = format_patch_strip_trailing_cr(line);
    let line_text = String::from_utf8_lossy(line);
    let (raw_key, _value) = line_text.split_once(':')?;
    let key = raw_key.trim_end_matches([' ', '\t']).trim();
    if key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return None;
    }
    let key_lower = key.to_ascii_lowercase();
    if configured_footer_keys.contains(&key_lower)
        || key_lower == "signed-off-by"
        || key_lower.ends_with("-by")
        || matches!(
            key_lower.as_str(),
            "bug" | "fixes" | "change-id" | "reviewed-id" | "cc"
        )
    {
        return Some(key_lower);
    }
    None
}

fn format_patch_footer_contains_signoff(lines: &[&[u8]], expected_suffix: &str) -> bool {
    lines.iter().any(|line| {
        let line = format_patch_strip_trailing_cr(line);
        let line_text = String::from_utf8_lossy(line);
        let Some(suffix) = line_text.strip_prefix("Signed-off-by:") else {
            return false;
        };
        suffix.trim() == expected_suffix.trim()
    })
}

fn format_patch_line_is_blank(line: &[u8]) -> bool {
    format_patch_strip_trailing_cr(line)
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t'))
}

fn format_patch_strip_trailing_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

const MAIL_HEADER_FOLD_WIDTH: usize = 78;
const MAIL_ENCODED_HEADER_FOLD_WIDTH: usize = 76;
const RFC2047_ENCODED_WORD_WRAPPER_LEN: usize = 12;

fn format_patch_author_header_value(signature: &[u8], encode_email_headers: bool) -> String {
    format!(
        "{} <{}>",
        format_patch_display_name(signature_name(signature).as_ref(), encode_email_headers),
        signature_email(signature)
    )
}

fn format_patch_display_name(name: &str, encode_email_headers: bool) -> String {
    if encode_email_headers && !name.is_ascii() {
        return format_patch_encode_rfc2047_q_words(
            name,
            MAIL_ENCODED_HEADER_FOLD_WIDTH - "From: ".len(),
        )
        .join(" ");
    }
    if !name.is_ascii() {
        return name.to_owned();
    }
    if format_patch_needs_rfc822_quotes(name) {
        return format!("\"{}\"", format_patch_escape_quoted_string(name));
    }
    name.to_owned()
}

fn format_patch_needs_rfc822_quotes(value: &str) -> bool {
    value.chars().any(|ch| {
        !matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | ' ' | '!' | '#'..='\''
            | '*' | '+' | '-' | '/' | '=' | '?' | '^' | '_' | '`' | '{' | '|' | '}' | '~')
    })
}

fn format_patch_escape_quoted_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | '"') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn format_patch_encode_rfc2047_q(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'!'
            | b'*'
            | b'+'
            | b'-'
            | b'/'
            | b'='
            | b'_' => encoded.push(char::from(*byte)),
            b' ' => encoded.push_str("=20"),
            _ => {
                use std::fmt::Write as _;
                let _ = write!(&mut encoded, "={byte:02X}");
            }
        }
    }
    format!("=?UTF-8?q?{encoded}?=")
}

fn format_patch_encode_rfc2047_q_piece(byte: u8, out: &mut String) {
    match byte {
        b'A'..=b'Z'
        | b'a'..=b'z'
        | b'0'..=b'9'
        | b'!'
        | b'*'
        | b'+'
        | b'-'
        | b'/'
        | b'='
        | b'_' => out.push(char::from(byte)),
        b' ' => out.push_str("=20"),
        _ => {
            use std::fmt::Write as _;
            let _ = write!(out, "={byte:02X}");
        }
    }
}

fn format_patch_encode_rfc2047_q_char_len(ch: char) -> usize {
    let mut encoded_len = 0;
    let mut buf = [0_u8; 4];
    for byte in ch.encode_utf8(&mut buf).as_bytes() {
        encoded_len += match *byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'!'
            | b'*'
            | b'+'
            | b'-'
            | b'/'
            | b'='
            | b'_' => 1,
            b' ' => 3,
            _ => 3,
        };
    }
    encoded_len
}

fn format_patch_encode_rfc2047_q_words(value: &str, first_line_width: usize) -> Vec<String> {
    let continuation_width = MAIL_ENCODED_HEADER_FOLD_WIDTH - 1;
    let mut words = Vec::new();
    let mut remaining = value.chars().peekable();
    let mut current_width = first_line_width;
    while remaining.peek().is_some() {
        let payload_limit = current_width.saturating_sub(RFC2047_ENCODED_WORD_WRAPPER_LEN);
        let mut payload = String::new();
        let mut payload_len = 0;
        while let Some(&ch) = remaining.peek() {
            let char_len = format_patch_encode_rfc2047_q_char_len(ch);
            if !payload.is_empty() && payload_len + char_len > payload_limit {
                break;
            }
            let mut buf = [0_u8; 4];
            for byte in ch.encode_utf8(&mut buf).as_bytes() {
                format_patch_encode_rfc2047_q_piece(*byte, &mut payload);
            }
            payload_len += char_len;
            remaining.next();
        }
        if payload.is_empty() {
            let ch = remaining.next().expect("rfc2047 char");
            let mut buf = [0_u8; 4];
            for byte in ch.encode_utf8(&mut buf).as_bytes() {
                format_patch_encode_rfc2047_q_piece(*byte, &mut payload);
            }
        }
        words.push(format!("=?UTF-8?q?{payload}?="));
        current_width = continuation_width;
    }
    words
}

fn write_format_patch_subject_header<W: Write>(
    out: &mut W,
    prefix: &str,
    subject: &str,
    encode_email_headers: bool,
) -> Result<()> {
    write!(out, "Subject: {prefix}")?;
    let current_len = "Subject: ".len() + prefix.len();
    if encode_email_headers && !subject.is_ascii() {
        let words = format_patch_encode_rfc2047_q_words(
            subject,
            MAIL_ENCODED_HEADER_FOLD_WIDTH - current_len,
        );
        write_folded_words_with_current_len(out, &words, current_len)?;
    } else {
        write_folded_tokens_with_current_len(out, &format_patch_fold_tokens(subject), current_len)?;
    }
    Ok(())
}

fn write_format_patch_folded_header<W: Write>(out: &mut W, name: &str, value: &str) -> Result<()> {
    write!(out, "{name}: ")?;
    write_folded_tokens_with_current_len(out, &format_patch_fold_tokens(value), name.len() + 2)
}

fn format_patch_fold_tokens(value: &str) -> Vec<&str> {
    value.split(' ').filter(|part| !part.is_empty()).collect()
}

fn write_folded_tokens_with_current_len<W: Write>(
    out: &mut W,
    tokens: &[&str],
    initial_len: usize,
) -> Result<()> {
    let mut current_len = initial_len;
    let mut line_has_tokens = false;
    for token in tokens {
        let additional = if line_has_tokens {
            1 + token.len()
        } else {
            token.len()
        };
        if line_has_tokens && current_len + additional > MAIL_HEADER_FOLD_WIDTH {
            writeln!(out)?;
            write!(out, " ")?;
            current_len = 1;
            line_has_tokens = false;
        }
        if line_has_tokens {
            write!(out, " ")?;
            current_len += 1;
        }
        write!(out, "{token}")?;
        current_len += token.len();
        line_has_tokens = true;
    }
    writeln!(out)?;
    Ok(())
}

fn write_folded_words_with_current_len<W: Write>(
    out: &mut W,
    words: &[String],
    initial_len: usize,
) -> Result<()> {
    let mut current_len = initial_len;
    let mut first = true;
    for word in words {
        let additional = if first { word.len() } else { 1 + word.len() };
        if !first && current_len + additional > MAIL_ENCODED_HEADER_FOLD_WIDTH {
            writeln!(out)?;
            write!(out, " ")?;
            current_len = 1;
            first = true;
        }
        if !first {
            write!(out, " ")?;
            current_len += 1;
        }
        write!(out, "{word}")?;
        current_len += word.len();
        first = false;
    }
    writeln!(out)?;
    Ok(())
}

fn write_format_patch_footer<W: Write>(out: &mut W, signature: Option<&str>) -> Result<()> {
    if let Some(signature) = signature {
        writeln!(out, "-- ")?;
        write!(out, "{signature}")?;
        if !signature.ends_with('\n') {
            writeln!(out)?;
        }
        writeln!(out)?;
    }
    Ok(())
}

fn write_format_patch_base_information<W: Write>(
    out: &mut W,
    base_information: &str,
    leading_blank_line: bool,
) -> Result<()> {
    if leading_blank_line {
        writeln!(out)?;
    }
    writeln!(out, "{base_information}")?;
    Ok(())
}

fn write_format_patch_appendix<W: Write>(
    out: &mut W,
    appendix: &str,
    leading_blank_line: bool,
) -> Result<()> {
    if appendix.is_empty() {
        return Ok(());
    }
    if leading_blank_line {
        writeln!(out)?;
    }
    out.write_all(appendix.as_bytes())?;
    if !appendix.ends_with('\n') {
        writeln!(out)?;
    }
    Ok(())
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
        ignore_blank_lines: false,
        whitespace_mode: DiffWhitespaceMode::None,
        relative_prefix: None,
        text: false,
        binary: true,
        binary_threshold: DEFAULT_CORE_BIG_FILE_THRESHOLD_BYTES,
        irreversible_delete: false,
        submodule_format: SubmoduleDiffFormat::Short,
        color: false,
        emit_hunk_headers: true,
        word_diff_regex: None,
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

fn format_patch_subject(message: &[u8], keep_subject: bool) -> String {
    let subject = commit_subject_view(message);
    if keep_subject {
        return subject.into_owned();
    }
    let first_paragraph = message
        .split(|byte| *byte == b'\n')
        .map(format_patch_strip_trailing_cr)
        .take_while(|line| !line.is_empty())
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect::<Vec<_>>();
    if first_paragraph.is_empty() {
        return subject.into_owned();
    }
    first_paragraph.join(" ")
}

fn format_patch_subject_prefix_label(prefix: &str) -> &str {
    prefix
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

fn write_commit_message_body<W: Write>(out: &mut W, message: &[u8], mboxrd: bool) -> Result<()> {
    let body = commit_message_body_view(message);
    if body.is_empty() {
        return Ok(());
    }
    if !mboxrd {
        out.write_all(body)?;
        if !body.ends_with(b"\n") {
            writeln!(out)?;
        }
        return Ok(());
    }
    for line in body.split_inclusive(|byte| *byte == b'\n') {
        let (content, has_newline) = line
            .strip_suffix(b"\n")
            .map(|content| (content, true))
            .unwrap_or((line, false));
        let mut content = content;
        while content
            .last()
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            content = &content[..content.len() - 1];
        }
        let quote_prefix_len = content.iter().take_while(|byte| **byte == b'>').count();
        if content[quote_prefix_len..].starts_with(b"From ") {
            out.write_all(&content[..quote_prefix_len])?;
            out.write_all(b">")?;
            out.write_all(&content[quote_prefix_len..])?;
        } else {
            out.write_all(content)?;
        }
        if has_newline {
            writeln!(out)?;
        }
    }
    if !body.ends_with(b"\n") {
        writeln!(out)?;
    }
    Ok(())
}

pub(crate) fn format_patch_filename_with_suffix(
    number: usize,
    subject: &str,
    suffix: &str,
) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for ch in subject.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_') {
            slug.push(ch);
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }
    let slug = slug.trim_matches(|ch| ch == '-' || ch == '.');
    let slug = if slug.is_empty() { "patch" } else { slug };
    format!("{number:04}-{slug}{suffix}")
}

pub(crate) fn format_patch_output_filename(
    number: usize,
    subject: &str,
    context: &FormatPatchContext<'_>,
) -> String {
    let mut filename = format_patch_filename_with_suffix(number, subject, context.suffix);
    if let Some(reroll_count) = context.reroll_count {
        filename = format!(
            "v{}-{filename}",
            sanitize_reroll_count_for_filename(reroll_count)
        );
    }
    if let Some(limit) = context.filename_max_length {
        let effective_limit = limit.saturating_sub(1);
        if filename.len() > effective_limit && effective_limit > context.suffix.len() {
            let suffix = context.suffix;
            let slug_marker = format!("{number:04}-");
            let slug_start = filename
                .find(&slug_marker)
                .map(|index| index + slug_marker.len())
                .or_else(|| filename.find('-').map(|index| index + 1))
                .unwrap_or(0);
            let prefix = &filename[..slug_start];
            let slug = filename[slug_start..].strip_suffix(suffix).unwrap_or("");
            let available = effective_limit.saturating_sub(prefix.len() + suffix.len());
            let truncated = slug.chars().take(available).collect::<String>();
            let truncated = truncated.trim_matches(|ch| ch == '-' || ch == '.');
            filename = format!(
                "{prefix}{}{suffix}",
                if truncated.is_empty() {
                    "patch"
                } else {
                    truncated
                }
            );
        }
    }
    filename
}

fn sanitize_reroll_count_for_filename(reroll_count: &str) -> String {
    let components = reroll_count
        .split('/')
        .filter_map(sanitize_reroll_count_component)
        .collect::<Vec<_>>();
    if components.is_empty() {
        "1".to_owned()
    } else {
        components.join("-")
    }
}

fn sanitize_reroll_count_component(component: &str) -> Option<String> {
    if component.is_empty() {
        return None;
    }
    let mut sanitized = String::new();
    let mut last_was_dash = false;
    let mut last_was_dot = false;
    for ch in component.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch);
            last_was_dash = false;
            last_was_dot = false;
        } else if ch == '.' {
            if !last_was_dot {
                sanitized.push('.');
                last_was_dot = true;
            }
            last_was_dash = false;
        } else if !last_was_dash {
            sanitized.push('-');
            last_was_dash = true;
            last_was_dot = false;
        }
    }
    let sanitized = sanitized.trim_matches('-').to_owned();
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

fn normalize_message_id_header(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with('<') && trimmed.ends_with('>') {
        trimmed.to_owned()
    } else {
        format!("<{trimmed}>")
    }
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
    let display = match path.strip_prefix(&cwd) {
        Ok(relative) => relative.display().to_string(),
        Err(_) => match path.strip_prefix(&repo.root) {
            Ok(relative) => relative.display().to_string(),
            Err(_) => path.display().to_string(),
        },
    };
    Ok(git_path_output_string(display))
}

#[cfg(windows)]
fn git_path_output_string(value: String) -> String {
    let value = value.replace('\\', "/");
    value.strip_prefix("./").unwrap_or(&value).to_owned()
}

#[cfg(not(windows))]
fn git_path_output_string(value: String) -> String {
    value.strip_prefix("./").unwrap_or(&value).to_owned()
}

pub(crate) fn print_name_status_entries(
    entries: &[zmin_git_core::IndexDiffEntry],
    relative_prefix: Option<&[u8]>,
    nul_terminated: bool,
) -> Result<()> {
    let stdout = io::stdout();
    let stdout = stdout.lock();
    let mut out = io::BufWriter::new(stdout);
    write_name_status_entries_buffered(&mut out, entries, relative_prefix, nul_terminated)
}

pub(crate) fn write_name_status_entries_buffered<W: Write>(
    out: &mut W,
    entries: &[zmin_git_core::IndexDiffEntry],
    relative_prefix: Option<&[u8]>,
    nul_terminated: bool,
) -> Result<()> {
    for entry in entries {
        if matches!(
            entry.status,
            IndexDiffStatus::Renamed | IndexDiffStatus::Copied
        ) {
            if nul_terminated {
                write!(
                    out,
                    "{}\0{}\0{}\0",
                    diff_entry_status_name(entry),
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    diff_display_path(&entry.path, relative_prefix)
                )?;
            } else {
                writeln!(
                    out,
                    "{}\t{}\t{}",
                    diff_entry_status_name(entry),
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    diff_display_path(&entry.path, relative_prefix)
                )?;
            }
        } else if nul_terminated {
            let path = if entry.status == IndexDiffStatus::Modified {
                diff_entry_old_path(entry)
            } else {
                &entry.path
            };
            write!(
                out,
                "{}\0{}\0",
                diff_entry_status_name(entry),
                diff_display_path(path, relative_prefix)
            )?;
        } else {
            let path = if entry.status == IndexDiffStatus::Modified {
                diff_entry_old_path(entry)
            } else {
                &entry.path
            };
            writeln!(
                out,
                "{}\t{}",
                diff_entry_status_name(entry),
                diff_display_path(path, relative_prefix)
            )?;
        }
    }
    Ok(())
}

pub(crate) fn diff_entry_status_name(entry: &zmin_git_core::IndexDiffEntry) -> String {
    match entry.status {
        IndexDiffStatus::Renamed => format!("R{:03}", entry.similarity.unwrap_or(100)),
        IndexDiffStatus::Copied => format!("C{:03}", entry.similarity.unwrap_or(100)),
        _ => entry.status.name_status().to_owned(),
    }
}

pub(crate) fn print_name_only_entries(
    entries: &[zmin_git_core::IndexDiffEntry],
    relative_prefix: Option<&[u8]>,
    nul_terminated: bool,
) -> Result<()> {
    let stdout = io::stdout();
    let stdout = stdout.lock();
    let mut out = io::BufWriter::new(stdout);
    write_name_only_entries_buffered(&mut out, entries, relative_prefix, nul_terminated)
}

pub(crate) fn write_name_only_entries_buffered<W: Write>(
    out: &mut W,
    entries: &[zmin_git_core::IndexDiffEntry],
    relative_prefix: Option<&[u8]>,
    nul_terminated: bool,
) -> Result<()> {
    for entry in entries {
        if nul_terminated {
            write!(out, "{}\0", diff_display_path(&entry.path, relative_prefix))?;
        } else {
            writeln!(out, "{}", diff_display_path(&entry.path, relative_prefix))?;
        }
    }
    Ok(())
}

pub(crate) fn diff_entry_old_path(entry: &zmin_git_core::IndexDiffEntry) -> &[u8] {
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
    entry: &'a zmin_git_core::IndexDiffEntry,
    relative_prefix: Option<&'b [u8]>,
) -> Cow<'a, str> {
    if matches!(
        entry.status,
        IndexDiffStatus::Renamed | IndexDiffStatus::Copied
    ) || (entry.status == IndexDiffStatus::Modified && entry.old_path.is_some())
    {
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
    entries: &[zmin_git_core::IndexDiffEntry],
    options: RawPrintOptions<'_>,
) -> Result<()> {
    let stdout = io::stdout();
    let stdout = stdout.lock();
    let mut out = io::BufWriter::new(stdout);
    write_raw_entries_buffered(&mut out, context, entries, options)
}

fn write_raw_entries_buffered<W: Write>(
    out: &mut W,
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: RawPrintOptions<'_>,
) -> Result<()> {
    let RawPrintOptions {
        abbrev_len,
        relative_prefix,
        nul_terminated,
    } = options;
    let abbrev_len = match abbrev_len {
        Some(length) => length,
        None => default_raw_abbrev_len_for_index_entries(context, entries)?,
    };
    let worktree_index = (context.old_source == DiffSideSource::WorktreeOrIndex
        || context.new_source == DiffSideSource::WorktreeOrIndex)
        .then(|| read_repo_index(context.repo))
        .transpose()?;
    for entry in entries {
        let old_entry = find_index_entry(context.old_index, diff_entry_old_path(entry));
        let new_entry = find_index_entry(context.new_index, &entry.path);
        let old_mode = old_entry
            .map(|entry| index_mode_octal(entry.mode))
            .unwrap_or("000000");
        let new_mode = new_entry
            .map(|entry| index_mode_octal(entry.mode))
            .unwrap_or("000000");
        let old_id = diff_raw_side_object_id(
            context,
            context.old_source,
            old_entry,
            worktree_index.as_ref(),
            abbrev_len,
        )?;
        let new_id = diff_raw_side_object_id(
            context,
            context.new_source,
            new_entry,
            worktree_index.as_ref(),
            abbrev_len,
        )?;
        if matches!(
            entry.status,
            IndexDiffStatus::Renamed | IndexDiffStatus::Copied
        ) {
            if nul_terminated {
                write!(
                    out,
                    ":{old_mode} {new_mode} {old_id} {new_id} {}\0{}\0{}\0",
                    diff_entry_status_name(entry),
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    diff_display_path(&entry.path, relative_prefix)
                )?;
            } else {
                writeln!(
                    out,
                    ":{old_mode} {new_mode} {old_id} {new_id} {}\t{}\t{}",
                    diff_entry_status_name(entry),
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    diff_display_path(&entry.path, relative_prefix)
                )?;
            }
        } else if nul_terminated {
            let path = if entry.status == IndexDiffStatus::Modified {
                diff_entry_old_path(entry)
            } else {
                &entry.path
            };
            write!(
                out,
                ":{old_mode} {new_mode} {old_id} {new_id} {}\0{}\0",
                diff_entry_status_name(entry),
                diff_display_path(path, relative_prefix)
            )?;
        } else {
            let path = if entry.status == IndexDiffStatus::Modified {
                diff_entry_old_path(entry)
            } else {
                &entry.path
            };
            writeln!(
                out,
                ":{old_mode} {new_mode} {old_id} {new_id} {}\t{}",
                diff_entry_status_name(entry),
                diff_display_path(path, relative_prefix)
            )?;
        }
    }
    Ok(())
}

fn default_raw_abbrev_len_for_index_entries(
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
) -> Result<usize> {
    let _trace = phase_trace("show.raw.default_raw_abbrev_len_for_index_entries");
    if !matches!(context.old_source, DiffSideSource::Index)
        || !matches!(context.new_source, DiffSideSource::Index)
    {
        return default_abbrev_len(context.store);
    }
    let mut ids = Vec::with_capacity(entries.len().saturating_mul(2));
    for entry in entries {
        if let Some(old_entry) = find_index_entry(context.old_index, diff_entry_old_path(entry)) {
            ids.push(old_entry.id.clone());
        }
        if let Some(new_entry) = find_index_entry(context.new_index, &entry.path) {
            ids.push(new_entry.id.clone());
        }
    }
    default_abbrev_len_for_ids(context.store, &ids)
}

fn default_raw_abbrev_len_for_tree_entries(
    store: &LooseObjectStore,
    entries: &[zmin_git_core::TreeDiffEntry],
) -> Result<usize> {
    let _trace = phase_trace("show.raw.default_raw_abbrev_len_for_tree_entries");
    let mut ids = Vec::with_capacity(entries.len().saturating_mul(2));
    for entry in entries {
        if let Some(old_entry) = entry.old_entry.as_ref() {
            ids.push(old_entry.id.clone());
        }
        if let Some(new_entry) = entry.new_entry.as_ref() {
            ids.push(new_entry.id.clone());
        }
    }
    default_abbrev_len_for_ids(store, &ids)
}

fn write_raw_entries_to<W: Write>(
    out: &mut W,
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    abbrev_len: usize,
    relative_prefix: Option<&[u8]>,
) -> Result<()> {
    let worktree_index = (context.old_source == DiffSideSource::WorktreeOrIndex
        || context.new_source == DiffSideSource::WorktreeOrIndex)
        .then(|| read_repo_index(context.repo))
        .transpose()?;
    for entry in entries {
        let old_entry = find_index_entry(context.old_index, diff_entry_old_path(entry));
        let new_entry = find_index_entry(context.new_index, &entry.path);
        let old_mode = old_entry
            .map(|entry| index_mode_octal(entry.mode))
            .unwrap_or("000000");
        let new_mode = new_entry
            .map(|entry| index_mode_octal(entry.mode))
            .unwrap_or("000000");
        let old_id = diff_raw_side_object_id(
            context,
            context.old_source,
            old_entry,
            worktree_index.as_ref(),
            abbrev_len,
        )?;
        let new_id = diff_raw_side_object_id(
            context,
            context.new_source,
            new_entry,
            worktree_index.as_ref(),
            abbrev_len,
        )?;
        if matches!(
            entry.status,
            IndexDiffStatus::Renamed | IndexDiffStatus::Copied
        ) {
            writeln!(
                out,
                ":{old_mode} {new_mode} {old_id} {new_id} {}\t{}\t{}",
                diff_entry_status_name(entry),
                diff_display_path(diff_entry_old_path(entry), relative_prefix),
                diff_display_path(&entry.path, relative_prefix)
            )?;
        } else {
            let path = if entry.status == IndexDiffStatus::Modified {
                diff_entry_old_path(entry)
            } else {
                &entry.path
            };
            writeln!(
                out,
                ":{old_mode} {new_mode} {old_id} {new_id} {}\t{}",
                diff_entry_status_name(entry),
                diff_display_path(path, relative_prefix)
            )?;
        }
    }
    Ok(())
}

fn diff_raw_side_object_id(
    context: &DiffIndexContext<'_>,
    source: DiffSideSource,
    entry: Option<&IndexEntry>,
    worktree_index: Option<&GitIndex>,
    abbrev_len: usize,
) -> Result<String> {
    let Some(entry) = entry else {
        return Ok(diff_raw_zero_object_id_len(abbrev_len));
    };
    if source == DiffSideSource::Index || entry.mode == IndexMode::Gitlink {
        return Ok(diff_raw_object_id_len(&entry.id, abbrev_len));
    }
    let content = read_diff_side_content(context.repo, context.store, entry, source)?;
    let id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content);
    let Some(index_entry) = worktree_index.and_then(|index| find_index_entry(index, &entry.path))
    else {
        return Ok(diff_raw_zero_object_id_len(abbrev_len));
    };
    let index_matches_worktree = index_entry.id == id
        && index_entry.mode == entry.mode
        && worktree_index_entry_stat_matches(context.repo, index_entry)?;
    if index_matches_worktree {
        Ok(diff_raw_object_id_len(&id, abbrev_len))
    } else {
        Ok(diff_raw_zero_object_id_len(abbrev_len))
    }
}

fn worktree_index_entry_stat_matches(repo: &GitRepo, entry: &IndexEntry) -> Result<bool> {
    let path = repo
        .root
        .join(String::from_utf8_lossy(&entry.path).as_ref());
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(index_entry_stat_matches(&metadata, entry)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CliError::Io(error)),
    }
}

pub(crate) fn short_zero_object_id_len(len: usize) -> String {
    "0".repeat(len)
}

pub(crate) fn diff_raw_object_id_len(id: &ObjectId, len: usize) -> String {
    let mut value = short_object_id_len(id, len);
    if diff_print_sha1_ellipsis_enabled() && len < id.hex_len() {
        value.push_str("...");
    }
    value
}

pub(crate) fn diff_raw_zero_object_id_len(len: usize) -> String {
    let mut value = short_zero_object_id_len(len);
    if diff_print_sha1_ellipsis_enabled() && len < GitHashAlgorithm::Sha1.digest_len() * 2 {
        value.push_str("...");
    }
    value
}

fn diff_print_sha1_ellipsis_enabled() -> bool {
    std::env::var("GIT_PRINT_SHA1_ELLIPSIS")
        .map(|value| value.eq_ignore_ascii_case("yes"))
        .unwrap_or(false)
}

pub(crate) fn print_summary_entries(
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
    relative_prefix: Option<&[u8]>,
) -> Result<()> {
    let mut out = io::stdout().lock();
    write_summary_entries(&mut out, old_index, new_index, entries, relative_prefix)
}

pub(crate) fn write_summary_entries<W: Write>(
    out: &mut W,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
    relative_prefix: Option<&[u8]>,
) -> Result<()> {
    for entry in entries {
        let old_entry = find_index_entry(old_index, diff_entry_old_path(entry));
        let new_entry = find_index_entry(new_index, &entry.path);
        let path = diff_display_path(&entry.path, relative_prefix);
        match (entry.status, old_entry, new_entry) {
            (IndexDiffStatus::Copied, Some(_), Some(_)) => {
                writeln!(
                    out,
                    " copy {} => {} (100%)",
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    path
                )?;
            }
            (IndexDiffStatus::Renamed, Some(_), Some(_)) => {
                writeln!(
                    out,
                    " rename {} => {} (100%)",
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    path
                )?;
            }
            (IndexDiffStatus::Added, _, Some(new_entry)) => {
                writeln!(
                    out,
                    " create mode {} {path}",
                    index_mode_octal(new_entry.mode)
                )?;
            }
            (IndexDiffStatus::Deleted, Some(old_entry), _) => {
                writeln!(
                    out,
                    " delete mode {} {path}",
                    index_mode_octal(old_entry.mode)
                )?;
            }
            (IndexDiffStatus::Modified, Some(old_entry), Some(new_entry))
                if old_entry.mode != new_entry.mode =>
            {
                writeln!(
                    out,
                    " mode change {} => {} {path}",
                    index_mode_octal(old_entry.mode),
                    index_mode_octal(new_entry.mode)
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct DiffStatRow {
    path: String,
    compact_summary: Option<&'static str>,
    old_bytes: usize,
    new_bytes: usize,
    insertions: usize,
    deletions: usize,
    dirstat_changes: usize,
    binary: bool,
}

struct DiffStatIndexedRow {
    entry_index: usize,
    row: DiffStatRow,
}

struct DiffStatBlobCache<'a> {
    packed_first_store: zmin_git_core::loose::PackedFirstObjectStore<'a>,
    shared_blobs: Option<&'a HashMap<ObjectId, Arc<[u8]>>>,
    cached_blobs: HashMap<ObjectId, Arc<[u8]>>,
}

impl<'a> DiffStatBlobCache<'a> {
    fn new(store: &'a LooseObjectStore) -> Self {
        Self::with_shared(store, None)
    }

    fn with_shared(
        store: &'a LooseObjectStore,
        shared_blobs: Option<&'a HashMap<ObjectId, Arc<[u8]>>>,
    ) -> Self {
        Self {
            packed_first_store: store.packed_first(),
            shared_blobs,
            cached_blobs: HashMap::new(),
        }
    }

    fn index_blob_content(&mut self, entry: &IndexEntry) -> Result<Arc<[u8]>> {
        if entry.mode == IndexMode::Gitlink {
            return Ok(Arc::from([]));
        }
        if let Some(shared) = self.shared_blobs
            && let Some(blob) = shared.get(&entry.id)
        {
            return Ok(blob.clone());
        }
        if self.shared_blobs.is_some() {
            let object = self.packed_first_store.read_object_transient(&entry.id)?;
            if object.kind != GitObjectKind::Blob {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "diff index entry does not point to a blob".into(),
                });
            }
            return Ok(Arc::from(object.content));
        }
        if let Entry::Vacant(vacant) = self.cached_blobs.entry(entry.id.clone()) {
            let object = self.packed_first_store.read_object(&entry.id)?;
            if object.kind != GitObjectKind::Blob {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "diff index entry does not point to a blob".into(),
                });
            }
            vacant.insert(Arc::from(object.content));
        }
        Ok(self
            .cached_blobs
            .get(&entry.id)
            .expect("diff stat blob cache entry must exist")
            .clone())
    }
}

fn preload_diff_stat_shared_blobs(
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
) -> Result<HashMap<ObjectId, Arc<[u8]>>> {
    let _trace = phase_trace("diff_stat.preload_shared_blobs");
    let mut object_id_counts = HashMap::with_capacity(entries.len() * 2);
    for entry in entries {
        if let Some(index_entry) = find_index_entry(old_index, diff_entry_old_path(entry))
            && index_entry.mode != IndexMode::Gitlink
        {
            *object_id_counts
                .entry(index_entry.id.clone())
                .or_insert(0usize) += 1;
        }
        if let Some(index_entry) = find_index_entry(new_index, &entry.path)
            && index_entry.mode != IndexMode::Gitlink
        {
            *object_id_counts
                .entry(index_entry.id.clone())
                .or_insert(0usize) += 1;
        }
    }

    let object_ids = object_id_counts
        .into_iter()
        .filter_map(|(object_id, count)| (count > 1).then_some(object_id))
        .collect::<Vec<_>>();
    if object_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let workers = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
        .min(object_ids.len())
        .min(PARALLEL_DIFF_STAT_MAX_WORKERS);
    if workers <= 1 || object_ids.len() < PARALLEL_DIFF_STAT_MIN_ENTRIES {
        let packed_first_store = store.packed_first();
        let mut shared_blobs = HashMap::with_capacity(object_ids.len());
        for object_id in object_ids {
            let object = packed_first_store.read_object(&object_id)?;
            if object.kind != GitObjectKind::Blob {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "diff index entry does not point to a blob".into(),
                });
            }
            shared_blobs.insert(object_id, Arc::from(object.content));
        }
        return Ok(shared_blobs);
    }

    let chunk_len = object_ids.len().div_ceil(workers).max(1);
    let packed_first_store = store.packed_first();
    let chunked = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for chunk in object_ids.chunks(chunk_len) {
            let packed_first_store = &packed_first_store;
            handles.push(
                scope.spawn(move || -> Result<HashMap<ObjectId, Arc<[u8]>>> {
                    let mut chunk_blobs = HashMap::with_capacity(chunk.len());
                    for object_id in chunk {
                        let object = packed_first_store.read_object(object_id)?;
                        if object.kind != GitObjectKind::Blob {
                            return Err(CliError::Fatal {
                                code: 128,
                                message: "diff index entry does not point to a blob".into(),
                            });
                        }
                        chunk_blobs.insert(object_id.clone(), Arc::from(object.content));
                    }
                    Ok(chunk_blobs)
                }),
            );
        }
        let mut chunked = Vec::with_capacity(handles.len());
        for handle in handles {
            chunked.push(handle.join().map_err(|_| {
                CliError::Message("parallel diff stat preload worker panicked".into())
            })??);
        }
        Ok::<_, CliError>(chunked)
    })?;

    let mut shared_blobs = HashMap::with_capacity(object_ids.len());
    for chunk_blobs in chunked {
        shared_blobs.extend(chunk_blobs);
    }
    Ok(shared_blobs)
}

pub(crate) struct FormatPatchContext<'a> {
    pub(crate) repo: &'a GitRepo,
    pub(crate) store: &'a LooseObjectStore,
    pub(crate) abbrev_len: usize,
    pub(crate) patch_abbrev_len: usize,
    pub(crate) total: usize,
    pub(crate) nul_terminated: bool,
    pub(crate) no_prefix: bool,
    pub(crate) no_numbered: bool,
    pub(crate) numbered: bool,
    pub(crate) numbered_files: bool,
    pub(crate) attach: bool,
    pub(crate) inline: bool,
    pub(crate) cover_letter: bool,
    pub(crate) include_mime_headers: bool,
    pub(crate) mime_boundary: Option<&'a str>,
    pub(crate) mboxrd: bool,
    pub(crate) suffix: &'a str,
    pub(crate) subject_prefix: &'a str,
    pub(crate) reroll_count: Option<&'a str>,
    pub(crate) commit_list_format: Option<&'a str>,
    pub(crate) prelude_mode: FormatPatchPreludeMode,
    pub(crate) reverse: bool,
    pub(crate) order_file: Option<&'a Path>,
    pub(crate) skip_to: Option<&'a str>,
    pub(crate) rotate_to: Option<&'a str>,
    pub(crate) word_diff: WordDiffMode,
    pub(crate) word_diff_regex: Option<&'a str>,
    pub(crate) submodule_format: SubmoduleDiffFormat,
    pub(crate) unified_context: usize,
    pub(crate) thread: Option<&'a str>,
    pub(crate) extra_headers: &'a [String],
    pub(crate) in_reply_to: Option<&'a str>,
    pub(crate) sender_override: Option<&'a str>,
    pub(crate) body_from_override: bool,
    pub(crate) encode_email_headers: bool,
    pub(crate) message_id_timestamp: Option<i64>,
    pub(crate) notes_by_commit: &'a HashMap<ObjectId, Vec<FormatPatchNoteBlock>>,
    pub(crate) keep_subject: bool,
    pub(crate) number_offset: usize,
    pub(crate) filename_max_length: Option<usize>,
    pub(crate) signoff_line: Option<&'a str>,
    pub(crate) signature: Option<&'a str>,
    pub(crate) zero_commit: bool,
    pub(crate) cover_subject: Option<&'a str>,
    pub(crate) cover_blurb: Option<&'a str>,
    pub(crate) base_information: Option<&'a str>,
    pub(crate) appendix: Option<&'a str>,
    pub(crate) relative_prefix: Option<Vec<u8>>,
    pub(crate) pathspecs: &'a [Vec<u8>],
    pub(crate) rename_threshold: Option<u8>,
    pub(crate) copy_threshold: Option<u8>,
    pub(crate) find_copies_harder: bool,
}

impl FormatPatchContext<'_> {
    fn cover_letter_base_emitted(&self, number: usize) -> bool {
        self.cover_letter && self.base_information.is_some() && number == 1
    }

    fn notes_for(&self, id: &ObjectId) -> Option<&[FormatPatchNoteBlock]> {
        self.notes_by_commit.get(id).map(Vec::as_slice)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FormatPatchNoteBlock {
    pub(crate) label: Option<String>,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormatPatchPreludeMode {
    Diffstat,
    None,
    Raw,
    Numstat,
    Dirstat,
    DirstatByFile,
    Shortstat,
    Summary,
}

#[derive(Clone, Copy)]
pub(crate) struct FormatPatchEntry<'a> {
    pub(crate) id: &'a ObjectId,
    pub(crate) commit: &'a zmin_git_core::CommitObject,
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
    pub(crate) ignore_blank_lines: bool,
    pub(crate) compact_summary: bool,
    pub(crate) color: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct NumstatOptions<'a> {
    pub(crate) stat: DiffStatOptions<'a>,
    pub(crate) nul_terminated: bool,
}

pub(crate) fn print_stat_entries(
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
) -> Result<()> {
    print_stat_entries_with_whitespace(context, entries, options)
}

pub(crate) fn print_stat_entries_with_whitespace(
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
) -> Result<()> {
    let mut out = io::stdout().lock();
    write_stat_entries(out.by_ref(), context, entries, options)
}

pub(crate) fn write_stat_entries<W: Write>(
    out: &mut W,
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
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
        .map(diff_stat_row_display_path_len)
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
        let display_path = diff_stat_row_display_path(row);
        let path = compact_stat_path(&display_path, path_width);
        let path_padding = " ".repeat(path_width.saturating_sub(path.len()));
        if row.binary {
            writeln!(
                out,
                " {}{} | {:>change_width$} {} -> {} bytes",
                path, path_padding, "Bin", row.old_bytes, row.new_bytes
            )?;
            continue;
        }
        let changes = row.insertions + row.deletions;
        let graph = stat_graph(row.insertions, row.deletions, max_changes, graph_width);
        let graph = if options.color {
            colorize_stat_graph(&graph)
        } else {
            graph
        };
        if graph.is_empty() {
            writeln!(
                out,
                " {}{} | {:>change_width$}",
                path, path_padding, changes
            )?;
        } else {
            writeln!(
                out,
                " {}{} | {:>change_width$} {}",
                path, path_padding, changes, graph
            )?;
        }
    }
    write_diff_stat_summary(out, &rows)
}

fn colorize_stat_graph(graph: &str) -> String {
    let plus = graph.bytes().take_while(|byte| *byte == b'+').count();
    let minus = graph.len().saturating_sub(plus);
    let mut colored = String::new();
    if plus > 0 {
        colored.push_str("\x1b[32m");
        colored.push_str(&"+".repeat(plus));
        colored.push_str("\x1b[m");
    }
    if minus > 0 {
        colored.push_str("\x1b[31m");
        colored.push_str(&"-".repeat(minus));
        colored.push_str("\x1b[m");
    }
    colored
}

fn diff_stat_row_display_path(row: &DiffStatRow) -> String {
    match row.compact_summary {
        Some(summary) => format!("{}{}", row.path, summary),
        None => row.path.clone(),
    }
}

fn diff_stat_row_display_path_len(row: &DiffStatRow) -> usize {
    row.path.len() + row.compact_summary.map(str::len).unwrap_or(0)
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
    let mut plus = insertions;
    let mut minus = deletions;
    let changes = plus + minus;
    if changes == 0 {
        return String::new();
    }
    if graph_width <= max_changes {
        let mut total = scale_linear(changes, graph_width, max_changes);
        if total < 2 && plus > 0 && minus > 0 {
            total = 2;
        }
        if plus < minus {
            plus = scale_linear(plus, graph_width, max_changes);
            minus = total.saturating_sub(plus);
        } else {
            minus = scale_linear(minus, graph_width, max_changes);
            plus = total.saturating_sub(minus);
        }
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

fn scale_linear(changes: usize, width: usize, max_changes: usize) -> usize {
    if changes == 0 {
        return 0;
    }
    if max_changes <= width {
        return changes;
    }
    1 + (changes.saturating_mul(width.saturating_sub(1)) / max_changes)
}

pub(crate) fn print_shortstat_entries(
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
) -> Result<()> {
    let rows = diff_stat_rows_with_whitespace(context, entries, options)?;
    if !rows.is_empty() {
        print_diff_stat_summary(&rows);
    }
    Ok(())
}

fn write_shortstat_entries_to<W: Write>(
    out: &mut W,
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
) -> Result<()> {
    let rows = diff_stat_rows_with_whitespace(context, entries, options)?;
    if !rows.is_empty() {
        write_diff_stat_summary(out, &rows)?;
    }
    Ok(())
}

pub(crate) fn print_numstat_entries(
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: NumstatOptions<'_>,
) -> Result<()> {
    let stdout = io::stdout();
    let stdout = stdout.lock();
    let mut out = io::BufWriter::new(stdout);
    write_numstat_entries_buffered(&mut out, context, entries, options)
}

fn write_numstat_entries_buffered<W: Write>(
    out: &mut W,
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: NumstatOptions<'_>,
) -> Result<()> {
    let NumstatOptions {
        stat:
            DiffStatOptions {
                whitespace_mode,
                relative_prefix,
                ignore_matching_lines,
                ignore_blank_lines,
                compact_summary: _,
                color: _,
            },
        nul_terminated,
    } = options;
    let rows = diff_stat_indexed_rows_with_whitespace(
        context,
        entries,
        DiffStatOptions {
            whitespace_mode,
            relative_prefix,
            ignore_matching_lines,
            ignore_blank_lines,
            compact_summary: false,
            color: false,
        },
        false,
    )?;
    for DiffStatIndexedRow { entry_index, row } in rows {
        let entry = &entries[entry_index];
        if row.binary {
            if nul_terminated
                && matches!(
                    entry.status,
                    IndexDiffStatus::Renamed | IndexDiffStatus::Copied
                )
            {
                write!(
                    out,
                    "-\t-\t\0{}\0{}\0",
                    diff_display_path(diff_entry_old_path(entry), relative_prefix),
                    diff_display_path(&entry.path, relative_prefix)
                )?;
            } else if nul_terminated {
                write!(out, "-\t-\t{}\0", row.path)?;
            } else {
                writeln!(out, "-\t-\t{}", row.path)?;
            }
        } else if nul_terminated
            && matches!(
                entry.status,
                IndexDiffStatus::Renamed | IndexDiffStatus::Copied
            )
        {
            write!(
                out,
                "{}\t{}\t\0{}\0{}\0",
                row.insertions,
                row.deletions,
                diff_display_path(diff_entry_old_path(entry), relative_prefix),
                diff_display_path(&entry.path, relative_prefix)
            )?;
        } else if nul_terminated {
            write!(out, "{}\t{}\t{}\0", row.insertions, row.deletions, row.path)?;
        } else {
            writeln!(out, "{}\t{}\t{}", row.insertions, row.deletions, row.path)?;
        }
    }
    Ok(())
}

fn write_numstat_entries_to<W: Write>(
    out: &mut W,
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: NumstatOptions<'_>,
) -> Result<()> {
    let NumstatOptions {
        stat:
            DiffStatOptions {
                whitespace_mode,
                relative_prefix,
                ignore_matching_lines,
                ignore_blank_lines,
                compact_summary: _,
                color: _,
            },
        nul_terminated: _,
    } = options;
    let rows = diff_stat_indexed_rows_with_whitespace(
        context,
        entries,
        DiffStatOptions {
            whitespace_mode,
            relative_prefix,
            ignore_matching_lines,
            ignore_blank_lines,
            compact_summary: false,
            color: false,
        },
        false,
    )?;
    for DiffStatIndexedRow { entry_index, row } in rows {
        let entry = &entries[entry_index];
        if row.binary {
            writeln!(out, "-\t-\t{}", row.path)?;
        } else if matches!(
            entry.status,
            IndexDiffStatus::Renamed | IndexDiffStatus::Copied
        ) {
            writeln!(
                out,
                "{}\t{}\t{} => {}",
                row.insertions,
                row.deletions,
                diff_display_path(diff_entry_old_path(entry), relative_prefix),
                diff_display_path(&entry.path, relative_prefix)
            )?;
        } else {
            writeln!(out, "{}\t{}\t{}", row.insertions, row.deletions, row.path)?;
        }
    }
    Ok(())
}

pub(crate) fn print_dirstat_entries<W: Write>(
    out: &mut W,
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
    by_file: bool,
    cumulative: bool,
) -> Result<()> {
    let rows = diff_stat_indexed_rows_with_whitespace(context, entries, options, true)?
        .into_iter()
        .map(|indexed| indexed.row)
        .collect::<Vec<_>>();
    let total = rows
        .iter()
        .map(|row| {
            if by_file {
                1usize
            } else if row.binary {
                row.old_bytes.max(row.new_bytes)
            } else {
                row.dirstat_changes
            }
        })
        .sum::<usize>();
    if total == 0 {
        return Ok(());
    }
    let mut dirs = BTreeMap::<String, usize>::new();
    for row in &rows {
        let Some((dir, _)) = row.path.rsplit_once('/') else {
            continue;
        };
        let weight = if by_file {
            1usize
        } else if row.binary {
            row.old_bytes.max(row.new_bytes)
        } else {
            row.dirstat_changes
        };
        if weight == 0 {
            continue;
        }
        if cumulative {
            let mut current = Some(dir);
            while let Some(segment) = current {
                *dirs.entry(format!("{segment}/")).or_default() += weight;
                current = segment.rsplit_once('/').map(|(parent, _)| parent);
            }
        } else {
            *dirs.entry(format!("{dir}/")).or_default() += weight;
        }
    }
    let mut dirs = dirs.into_iter().collect::<Vec<_>>();
    dirs.sort_by(|(left_dir, left_weight), (right_dir, right_weight)| {
        right_weight
            .cmp(left_weight)
            .then_with(|| {
                right_dir
                    .matches('/')
                    .count()
                    .cmp(&left_dir.matches('/').count())
            })
            .then_with(|| left_dir.cmp(right_dir))
    });
    for (dir, weight) in dirs {
        let percent = ((weight as f64 * 1000.0) / total as f64).floor() / 10.0;
        if percent >= 3.0 {
            writeln!(out, "{percent:6.1}% {dir}")?;
        }
    }
    Ok(())
}

pub(crate) fn diff_stat_rows_with_whitespace(
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
) -> Result<Vec<DiffStatRow>> {
    Ok(
        diff_stat_indexed_rows_with_whitespace(context, entries, options, false)?
            .into_iter()
            .map(|indexed| indexed.row)
            .collect(),
    )
}

fn diff_stat_indexed_rows_with_whitespace(
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
    compute_dirstat_changes: bool,
) -> Result<Vec<DiffStatIndexedRow>> {
    if let Some(rows) =
        try_diff_stat_indexed_rows_parallel(context, entries, options, compute_dirstat_changes)?
    {
        return Ok(rows);
    }
    let mut rows = Vec::with_capacity(diff_stat_row_initial_capacity(entries.len()));
    let mut blob_cache = (context.old_source == DiffSideSource::Index
        && context.new_source == DiffSideSource::Index)
        .then(|| DiffStatBlobCache::new(context.store));
    for (entry_index, entry) in entries.iter().enumerate() {
        let row = diff_stat_row_with_whitespace(
            context,
            entry,
            options,
            blob_cache.as_mut(),
            compute_dirstat_changes,
        )?;
        if diff_stat_row_should_emit(context, entry, &row) {
            rows.push(DiffStatIndexedRow { entry_index, row });
        }
    }
    Ok(rows)
}

fn try_diff_stat_indexed_rows_parallel(
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    options: DiffStatOptions<'_>,
    compute_dirstat_changes: bool,
) -> Result<Option<Vec<DiffStatIndexedRow>>> {
    if !matches!(context.old_source, DiffSideSource::Index)
        || !matches!(context.new_source, DiffSideSource::Index)
        || entries.len() < PARALLEL_DIFF_STAT_MIN_ENTRIES
    {
        return Ok(None);
    }
    let workers = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
        .min(entries.len())
        .min(PARALLEL_DIFF_STAT_MAX_WORKERS);
    if workers <= 1 {
        return Ok(None);
    }
    let shared_blobs = if entries.len() >= PARALLEL_DIFF_STAT_SHARED_PRELOAD_MIN_ENTRIES {
        Some(preload_diff_stat_shared_blobs(
            context.store,
            context.old_index,
            context.new_index,
            entries,
        )?)
    } else {
        None
    };
    let chunk_ranges = diff_stat_parallel_chunk_ranges(context, entries, workers);
    let chunked_rows = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(chunk_ranges.len());
        for chunk_range in &chunk_ranges {
            let chunk_start = chunk_range.start;
            let chunk = &entries[chunk_range.clone()];
            let shared_blobs = shared_blobs.as_ref();
            handles.push(scope.spawn(move || -> Result<Vec<DiffStatIndexedRow>> {
                let chunk_context = DiffIndexContext {
                    repo: context.repo,
                    store: context.store,
                    old_index: context.old_index,
                    new_index: context.new_index,
                    old_source: DiffSideSource::Index,
                    new_source: DiffSideSource::Index,
                };
                let mut blob_cache = DiffStatBlobCache::with_shared(context.store, shared_blobs);
                let mut chunk_rows =
                    Vec::with_capacity(diff_stat_row_initial_capacity(chunk.len()));
                for (offset, entry) in chunk.iter().enumerate() {
                    let row = diff_stat_row_with_whitespace(
                        &chunk_context,
                        entry,
                        options,
                        Some(&mut blob_cache),
                        compute_dirstat_changes,
                    )?;
                    if diff_stat_row_should_emit(&chunk_context, entry, &row) {
                        chunk_rows.push(DiffStatIndexedRow {
                            entry_index: chunk_start + offset,
                            row,
                        });
                    }
                }
                Ok(chunk_rows)
            }));
        }
        let mut chunked_rows = Vec::with_capacity(handles.len());
        for handle in handles {
            chunked_rows.push(
                handle.join().map_err(|_| {
                    CliError::Message("parallel diff stat worker panicked".into())
                })??,
            );
        }
        Ok::<_, CliError>(chunked_rows)
    })?;
    let mut rows = Vec::with_capacity(diff_stat_row_initial_capacity(entries.len()));
    for mut chunk_rows in chunked_rows {
        rows.append(&mut chunk_rows);
    }
    Ok(Some(rows))
}

fn diff_stat_parallel_chunk_ranges(
    context: &DiffIndexContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
    workers: usize,
) -> Vec<Range<usize>> {
    let weights = entries
        .iter()
        .map(|entry| {
            let old_bytes = find_index_entry(context.old_index, diff_entry_old_path(entry))
                .map_or(0, |index_entry| u64::from(index_entry.size));
            let new_bytes = find_index_entry(context.new_index, &entry.path)
                .map_or(0, |index_entry| u64::from(index_entry.size));
            old_bytes.saturating_add(new_bytes)
        })
        .collect::<Vec<_>>();
    let total_bytes = weights.iter().copied().sum::<u64>();
    if total_bytes == 0 {
        let chunk_len = entries.len().div_ceil(workers).max(1);
        return (0..entries.len())
            .step_by(chunk_len)
            .map(|start| start..(start + chunk_len).min(entries.len()))
            .collect();
    }

    let mut ranges = Vec::with_capacity(workers);
    let mut start = 0usize;
    let mut range_bytes = 0u64;
    let mut remaining_bytes = total_bytes;
    for (index, weight) in weights.into_iter().enumerate() {
        let remaining_workers = workers.saturating_sub(ranges.len());
        if index > start && remaining_workers > 1 {
            let target = remaining_bytes.div_ceil(remaining_workers as u64);
            let undershoot = target.saturating_sub(range_bytes);
            let overshoot = range_bytes.saturating_add(weight).saturating_sub(target);
            if range_bytes >= target || undershoot <= overshoot {
                ranges.push(start..index);
                remaining_bytes = remaining_bytes.saturating_sub(range_bytes);
                start = index;
                range_bytes = 0;
            }
        }
        range_bytes = range_bytes.saturating_add(weight);
    }
    ranges.push(start..entries.len());
    ranges
}

fn diff_stat_row_should_emit(
    context: &DiffIndexContext<'_>,
    entry: &zmin_git_core::IndexDiffEntry,
    row: &DiffStatRow,
) -> bool {
    row.binary
        || row.insertions + row.deletions > 0
        || row.compact_summary.is_some()
        || diff_entry_has_metadata_change(context, entry)
}

fn diff_stat_row_with_whitespace(
    context: &DiffIndexContext<'_>,
    entry: &zmin_git_core::IndexDiffEntry,
    options: DiffStatOptions<'_>,
    mut blob_cache: Option<&mut DiffStatBlobCache<'_>>,
    compute_dirstat_changes: bool,
) -> Result<DiffStatRow> {
    let old_entry = find_index_entry(context.old_index, diff_entry_old_path(entry));
    let new_entry = find_index_entry(context.new_index, &entry.path);
    if let (Some(old_entry), Some(new_entry)) = (old_entry, new_entry)
        && old_entry.id == new_entry.id
    {
        return Ok(DiffStatRow {
            path: diff_entry_stat_path(entry, options.relative_prefix).into_owned(),
            compact_summary: diff_entry_compact_summary(
                options.compact_summary,
                entry,
                Some(old_entry),
                Some(new_entry),
            ),
            old_bytes: old_entry.size as usize,
            new_bytes: new_entry.size as usize,
            insertions: 0,
            deletions: 0,
            dirstat_changes: 0,
            binary: false,
        });
    }
    let old_content = match old_entry {
        Some(entry) => read_diff_side_content_for_stat(
            context.repo,
            context.store,
            entry,
            context.old_source,
            blob_cache.as_deref_mut(),
        )?,
        None => Arc::from([]),
    };
    let new_content = match new_entry {
        Some(entry) => read_diff_side_content_for_stat(
            context.repo,
            context.store,
            entry,
            context.new_source,
            blob_cache.as_deref_mut(),
        )?,
        None => Arc::from([]),
    };
    let (binary, insertions, deletions) = if old_entry.is_none() {
        let _trace = phase_trace("diff_stat.entry_line_counts.single_sided_added");
        diff_stat_single_sided_analysis(
            &new_content,
            true,
            options.ignore_matching_lines,
            options.ignore_blank_lines,
        )
    } else if new_entry.is_none() {
        let _trace = phase_trace("diff_stat.entry_line_counts.single_sided_deleted");
        diff_stat_single_sided_analysis(
            &old_content,
            false,
            options.ignore_matching_lines,
            options.ignore_blank_lines,
        )
    } else {
        let binary = {
            let _trace = phase_trace("diff_stat.entry_binary_detect");
            is_binary_content(&old_content) || is_binary_content(&new_content)
        };
        let (insertions, deletions) = if binary {
            (0, 0)
        } else {
            let _trace = phase_trace("diff_stat.entry_line_counts.diff");
            diff_line_counts_with_options(
                &old_content,
                &new_content,
                options.whitespace_mode,
                options.ignore_matching_lines,
                options.ignore_blank_lines,
            )
        };
        (binary, insertions, deletions)
    };
    Ok(DiffStatRow {
        path: diff_entry_stat_path(entry, options.relative_prefix).into_owned(),
        compact_summary: diff_entry_compact_summary(
            options.compact_summary,
            entry,
            old_entry,
            new_entry,
        ),
        old_bytes: old_content.len(),
        new_bytes: new_content.len(),
        insertions,
        deletions,
        dirstat_changes: if compute_dirstat_changes {
            diff_stat_row_dirstat_changes(old_entry, new_entry, &old_content, &new_content, binary)
        } else {
            0
        },
        binary,
    })
}

fn diff_stat_single_sided_counts(
    content: &[u8],
    inserted: bool,
    ignore_matching_lines: &[Regex],
    ignore_blank_lines: bool,
) -> (usize, usize) {
    let line_count = count_diff_lines(content);
    if line_count == 0 {
        return (0, 0);
    }
    if diff_stat_single_sided_change_is_ignored(content, ignore_matching_lines, ignore_blank_lines)
    {
        return (0, 0);
    }
    if inserted {
        (line_count, 0)
    } else {
        (0, line_count)
    }
}

fn diff_stat_single_sided_analysis(
    content: &[u8],
    inserted: bool,
    ignore_matching_lines: &[Regex],
    ignore_blank_lines: bool,
) -> (bool, usize, usize) {
    if ignore_matching_lines.is_empty() && !ignore_blank_lines {
        let (binary, line_count) = diff_stat_binary_and_line_count(content);
        if binary || line_count == 0 {
            return (binary, 0, 0);
        }
        return if inserted {
            (false, line_count, 0)
        } else {
            (false, 0, line_count)
        };
    }
    let binary = {
        let _trace = phase_trace("diff_stat.entry_binary_detect");
        is_binary_content(content)
    };
    if binary {
        return (true, 0, 0);
    }
    let (insertions, deletions) =
        diff_stat_single_sided_counts(content, inserted, ignore_matching_lines, ignore_blank_lines);
    (false, insertions, deletions)
}

fn diff_stat_binary_and_line_count(content: &[u8]) -> (bool, usize) {
    if content.is_empty() {
        return (false, 0);
    }
    let mut binary = false;
    let mut newline_count = 0usize;
    let binary_prefix_len = content.len().min(BINARY_DETECTION_BYTES);
    for (idx, byte) in content.iter().enumerate() {
        if idx < binary_prefix_len && *byte == 0 {
            binary = true;
        }
        if *byte == b'\n' {
            newline_count += 1;
        }
    }
    (
        binary,
        newline_count + usize::from(!content.ends_with(b"\n")),
    )
}

fn diff_stat_single_sided_change_is_ignored(
    content: &[u8],
    ignore_matching_lines: &[Regex],
    ignore_blank_lines: bool,
) -> bool {
    if ignore_matching_lines.is_empty() && !ignore_blank_lines {
        return false;
    }
    let mut saw_change = false;
    for line in split_diff_lines(content) {
        saw_change = true;
        if ignore_blank_lines && diff_ignore_blank_line(line) {
            continue;
        }
        if ignore_matching_lines
            .iter()
            .any(|pattern| pattern.is_match(strip_trailing_lf(line)))
        {
            continue;
        }
        return false;
    }
    saw_change
}

fn diff_stat_row_dirstat_changes(
    old_entry: Option<&IndexEntry>,
    new_entry: Option<&IndexEntry>,
    old_content: &[u8],
    new_content: &[u8],
    binary: bool,
) -> usize {
    match (old_entry, new_entry) {
        (Some(_), Some(_)) => {
            if binary {
                old_content.len().max(new_content.len())
            } else if old_content == new_content {
                1
            } else {
                diff_dirstat_changes(old_content, new_content, false).max(1)
            }
        }
        (Some(_), None) => old_content.len(),
        (None, Some(_)) => new_content.len(),
        (None, None) => 0,
    }
}

fn diff_dirstat_changes(old: &[u8], new: &[u8], binary: bool) -> usize {
    let src_counts = diff_dirstat_span_counts(old, !binary);
    let dst_counts = diff_dirstat_span_counts(new, !binary);
    let mut src_iter = src_counts.into_iter().peekable();
    let mut dst_iter = dst_counts.into_iter().peekable();
    let mut copied = 0usize;
    let mut added = 0usize;

    while let Some(&(src_hash, src_count)) = src_iter.peek() {
        while let Some(&(dst_hash, dst_count)) = dst_iter.peek() {
            if dst_hash >= src_hash {
                break;
            }
            added += dst_count;
            dst_iter.next();
        }

        let mut dst_count = 0usize;
        if let Some(&(dst_hash, count)) = dst_iter.peek()
            && dst_hash == src_hash
        {
            dst_count = count;
            dst_iter.next();
        }

        if src_count < dst_count {
            added += dst_count - src_count;
            copied += src_count;
        } else {
            copied += dst_count;
        }
        src_iter.next();
    }

    for (_, dst_count) in dst_iter {
        added += dst_count;
    }

    old.len().saturating_sub(copied) + added
}

fn diff_dirstat_span_counts(content: &[u8], text: bool) -> Vec<(u32, usize)> {
    const DIRSTAT_HASHBASE: u32 = 107_927;

    let mut counts = HashMap::<u32, usize>::new();
    let mut idx = 0usize;
    let mut chunk_len = 0usize;
    let mut accum1 = 0u32;
    let mut accum2 = 0u32;

    while idx < content.len() {
        let byte = content[idx];
        idx += 1;
        if text && byte == b'\r' && idx < content.len() && content[idx] == b'\n' {
            continue;
        }

        let old_accum1 = accum1;
        accum1 = (accum1 << 7) ^ (accum2 >> 25);
        accum2 = (accum2 << 7) ^ (old_accum1 >> 25);
        accum1 = accum1.wrapping_add(byte as u32);
        chunk_len += 1;

        if chunk_len < 64 && byte != b'\n' {
            continue;
        }

        let hash = (accum1.wrapping_add(accum2.wrapping_mul(0x61))) % DIRSTAT_HASHBASE;
        *counts.entry(hash).or_default() += chunk_len;
        chunk_len = 0;
        accum1 = 0;
        accum2 = 0;
    }

    if chunk_len > 0 {
        let hash = (accum1.wrapping_add(accum2.wrapping_mul(0x61))) % DIRSTAT_HASHBASE;
        *counts.entry(hash).or_default() += chunk_len;
    }

    let mut spans = counts.into_iter().collect::<Vec<_>>();
    spans.sort_unstable_by_key(|(hash, _)| *hash);
    spans
}

fn read_diff_side_content_for_stat(
    repo: &GitRepo,
    store: &LooseObjectStore,
    entry: &IndexEntry,
    source: DiffSideSource,
    blob_cache: Option<&mut DiffStatBlobCache<'_>>,
) -> Result<Arc<[u8]>> {
    match source {
        DiffSideSource::Index => {
            let _trace = phase_trace("diff_stat.entry_content.index");
            if let Some(cache) = blob_cache {
                return cache.index_blob_content(entry);
            }
            Ok(Arc::from(read_index_entry_content(store, entry)?))
        }
        DiffSideSource::WorktreeOrIndex => {
            let _trace = phase_trace("diff_stat.entry_content.worktree_or_index");
            Ok(Arc::from(read_worktree_or_index_entry_content(
                repo, store, entry,
            )?))
        }
    }
}

fn diff_entry_compact_summary(
    compact_summary: bool,
    entry: &zmin_git_core::IndexDiffEntry,
    old_entry: Option<&IndexEntry>,
    new_entry: Option<&IndexEntry>,
) -> Option<&'static str> {
    if !compact_summary {
        return None;
    }
    match entry.status {
        IndexDiffStatus::Added => Some(" (new)"),
        IndexDiffStatus::Deleted => Some(" (gone)"),
        _ => match (
            old_entry.map(|entry| entry.mode),
            new_entry.map(|entry| entry.mode),
        ) {
            (Some(IndexMode::File), Some(IndexMode::Executable)) => Some(" (mode +x)"),
            (Some(IndexMode::Executable), Some(IndexMode::File)) => Some(" (mode -x)"),
            _ => None,
        },
    }
}

pub(crate) fn diff_line_counts_with_options(
    old: &[u8],
    new: &[u8],
    whitespace_mode: DiffWhitespaceMode,
    ignore_matching_lines: &[Regex],
    ignore_blank_lines: bool,
) -> (usize, usize) {
    if matches!(whitespace_mode, DiffWhitespaceMode::None)
        && ignore_matching_lines.is_empty()
        && !ignore_blank_lines
    {
        return diff_line_counts_plain(old, new);
    }
    if ignore_matching_lines.is_empty() && !ignore_blank_lines {
        return diff_line_counts_without_structural_heuristics(old, new, whitespace_mode);
    }
    let old_lines = split_diff_lines(old);
    let new_lines = split_diff_lines(new);
    let ops = diff_line_ops_with_whitespace(&old_lines, &new_lines, whitespace_mode);
    let mut insertions = 0;
    let mut deletions = 0;
    for (start, end) in unified_hunk_ranges(&ops, 3, 0) {
        if hunk_ignored_by_matching_lines(
            &ops,
            start,
            end,
            ignore_matching_lines,
            ignore_blank_lines,
        ) {
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

fn diff_line_counts_without_structural_heuristics(
    old: &[u8],
    new: &[u8],
    whitespace_mode: DiffWhitespaceMode,
) -> (usize, usize) {
    let old_lines = split_diff_lines(old);
    let new_lines = split_diff_lines(new);
    let ops = diff_line_ops_with_whitespace_and_heuristics(
        &old_lines,
        &new_lines,
        whitespace_mode,
        false,
    );
    let mut insertions = 0;
    let mut deletions = 0;
    for op in ops {
        match op {
            DiffLineOp::Equal(_) => {}
            DiffLineOp::Delete(_) => deletions += 1,
            DiffLineOp::Insert(_) => insertions += 1,
        }
    }
    (insertions, deletions)
}

fn diff_line_counts_plain(old: &[u8], new: &[u8]) -> (usize, usize) {
    if old == new {
        return (0, 0);
    }
    if let Some(appended) = new.strip_prefix(old)
        && diff_stat_line_delta_extends_after_complete_lines(old, appended)
    {
        return (count_diff_lines(appended), 0);
    }
    if let Some(deleted) = old.strip_prefix(new)
        && diff_stat_line_delta_extends_after_complete_lines(new, deleted)
    {
        return (0, count_diff_lines(deleted));
    }
    if let Some(prepended) = new.strip_suffix(old)
        && diff_stat_line_delta_starts_with_complete_lines(prepended, old)
    {
        return (count_diff_lines(prepended), 0);
    }
    if let Some(deleted) = old.strip_suffix(new)
        && diff_stat_line_delta_starts_with_complete_lines(deleted, new)
    {
        return (0, count_diff_lines(deleted));
    }
    let old_lines = split_diff_lines(old);
    let new_lines = split_diff_lines(new);
    let common_prefix = old_lines
        .iter()
        .zip(new_lines.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let old_remaining = old_lines.len().saturating_sub(common_prefix);
    let new_remaining = new_lines.len().saturating_sub(common_prefix);
    let common_suffix = old_lines[common_prefix..]
        .iter()
        .rev()
        .zip(new_lines[common_prefix..].iter().rev())
        .take(old_remaining.min(new_remaining))
        .take_while(|(left, right)| left == right)
        .count();
    let old_lines = &old_lines[common_prefix..old_lines.len().saturating_sub(common_suffix)];
    let new_lines = &new_lines[common_prefix..new_lines.len().saturating_sub(common_suffix)];
    if old_lines.is_empty() {
        return (new_lines.len(), 0);
    }
    if new_lines.is_empty() {
        return (0, old_lines.len());
    }
    if old_lines.len() + new_lines.len() >= DIFF_LINE_COUNT_INTERN_MIN_LINES {
        return diff_line_counts_plain_interned(old_lines, new_lines);
    }
    let mut insertions = 0usize;
    let mut deletions = 0usize;
    for segment in capture_diff_slices(Algorithm::Myers, old_lines, new_lines) {
        match segment {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete { old_len, .. } => deletions += old_len,
            DiffOp::Insert { new_len, .. } => insertions += new_len,
            DiffOp::Replace {
                old_len, new_len, ..
            } => {
                deletions += old_len;
                insertions += new_len;
            }
        }
    }
    (insertions, deletions)
}

fn diff_stat_line_delta_extends_after_complete_lines(base: &[u8], delta: &[u8]) -> bool {
    delta.is_empty() || base.is_empty() || base.ends_with(b"\n")
}

fn diff_stat_line_delta_starts_with_complete_lines(delta: &[u8], remainder: &[u8]) -> bool {
    delta.is_empty() || remainder.is_empty() || delta.ends_with(b"\n")
}

fn diff_line_counts_plain_interned(old_lines: &[&[u8]], new_lines: &[&[u8]]) -> (usize, usize) {
    let mut next_id = 0u32;
    let mut line_ids = FxHashMap::<&[u8], u32>::with_capacity_and_hasher(
        old_lines.len() + new_lines.len(),
        Default::default(),
    );
    let mut old_ids = Vec::with_capacity(old_lines.len());
    let mut new_ids = Vec::with_capacity(new_lines.len());

    for &line in old_lines {
        let id = *line_ids.entry(line).or_insert_with(|| {
            let id = next_id;
            next_id = next_id.saturating_add(1);
            id
        });
        old_ids.push(id);
    }
    for &line in new_lines {
        let id = *line_ids.entry(line).or_insert_with(|| {
            let id = next_id;
            next_id = next_id.saturating_add(1);
            id
        });
        new_ids.push(id);
    }
    let mut insertions = 0usize;
    let mut deletions = 0usize;
    for segment in capture_diff_slices(Algorithm::Myers, old_ids.as_slice(), new_ids.as_slice()) {
        match segment {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete { old_len, .. } => deletions += old_len,
            DiffOp::Insert { new_len, .. } => insertions += new_len,
            DiffOp::Replace {
                old_len, new_len, ..
            } => {
                deletions += old_len;
                insertions += new_len;
            }
        }
    }
    (insertions, deletions)
}

pub(crate) fn print_diff_stat_summary(rows: &[DiffStatRow]) {
    let mut out = io::stdout().lock();
    let _ = write_diff_stat_summary(&mut out, rows);
}

fn write_diff_stat_summary<W: Write>(out: &mut W, rows: &[DiffStatRow]) -> Result<()> {
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
    writeln!(out, "{summary}")?;
    Ok(())
}

pub(crate) fn plural<'a>(value: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if value == 1 { singular } else { plural }
}

pub(crate) fn print_patch_entries(
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
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
    pub(crate) word_diff_regex: Option<String>,
    pub(crate) abbrev_len: Option<usize>,
    pub(crate) old_prefix: String,
    pub(crate) new_prefix: String,
    pub(crate) unified_context: usize,
    pub(crate) inter_hunk_context: usize,
    pub(crate) output_indicator_new: Option<u8>,
    pub(crate) output_indicator_old: Option<u8>,
    pub(crate) output_indicator_context: Option<u8>,
    pub(crate) ignore_matching_lines: Vec<Regex>,
    pub(crate) ignore_blank_lines: bool,
    pub(crate) whitespace_mode: DiffWhitespaceMode,
    pub(crate) relative_prefix: Option<Vec<u8>>,
    pub(crate) text: bool,
    pub(crate) binary: bool,
    pub(crate) irreversible_delete: bool,
    pub(crate) submodule_format: SubmoduleDiffFormat,
    pub(crate) color_mode: DiffColorMode,
    pub(crate) emit_hunk_headers: bool,
    pub(crate) line_prefix: Option<String>,
}

impl PatchFormatOptions {
    pub(crate) fn cached() -> Self {
        Self {
            old_source: DiffSideSource::Index,
            new_source: DiffSideSource::Index,
            word_diff: WordDiffMode::None,
            word_diff_regex: None,
            abbrev_len: None,
            old_prefix: "a/".to_owned(),
            new_prefix: "b/".to_owned(),
            unified_context: 3,
            inter_hunk_context: 0,
            output_indicator_new: Some(b'+'),
            output_indicator_old: Some(b'-'),
            output_indicator_context: Some(b' '),
            ignore_matching_lines: Vec::new(),
            ignore_blank_lines: false,
            whitespace_mode: DiffWhitespaceMode::None,
            relative_prefix: None,
            text: false,
            binary: false,
            irreversible_delete: false,
            submodule_format: SubmoduleDiffFormat::Short,
            color_mode: DiffColorMode::Never,
            emit_hunk_headers: true,
            line_prefix: None,
        }
    }

    pub(crate) fn worktree() -> Self {
        Self {
            old_source: DiffSideSource::Index,
            new_source: DiffSideSource::WorktreeOrIndex,
            word_diff: WordDiffMode::None,
            word_diff_regex: None,
            abbrev_len: None,
            old_prefix: "a/".to_owned(),
            new_prefix: "b/".to_owned(),
            unified_context: 3,
            inter_hunk_context: 0,
            output_indicator_new: Some(b'+'),
            output_indicator_old: Some(b'-'),
            output_indicator_context: Some(b' '),
            ignore_matching_lines: Vec::new(),
            ignore_blank_lines: false,
            whitespace_mode: DiffWhitespaceMode::None,
            relative_prefix: None,
            text: false,
            binary: false,
            irreversible_delete: false,
            submodule_format: SubmoduleDiffFormat::Short,
            color_mode: DiffColorMode::Never,
            emit_hunk_headers: true,
            line_prefix: None,
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

    pub(crate) fn with_line_prefix(mut self, line_prefix: Option<String>) -> Self {
        self.line_prefix = line_prefix;
        self
    }
}

pub(crate) fn write_patch_entries<W: Write>(
    out: &mut W,
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
    format: PatchFormatOptions,
) -> Result<()> {
    if let Some(prefix) = format.line_prefix.clone() {
        let mut prefixed = LinePrefixWriter::new(out, prefix.as_bytes());
        return write_patch_entries_unprefixed(
            &mut prefixed,
            repo,
            store,
            old_index,
            new_index,
            entries,
            format.with_line_prefix(None),
        );
    }
    write_patch_entries_unprefixed(out, repo, store, old_index, new_index, entries, format)
}

fn write_patch_entries_unprefixed<W: Write>(
    out: &mut W,
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
    format: PatchFormatOptions,
) -> Result<()> {
    let abbrev_len = format.abbrev_len.unwrap_or(default_auto_abbrev_len(store)?);
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
        ignore_blank_lines: format.ignore_blank_lines,
        whitespace_mode: format.whitespace_mode,
        relative_prefix: format.relative_prefix.as_deref(),
        text: format.text,
        binary: format.binary,
        binary_threshold: core_big_file_threshold(repo)?,
        irreversible_delete: format.irreversible_delete,
        submodule_format: format.submodule_format,
        color: format.color_mode.enabled(),
        emit_hunk_headers: format.emit_hunk_headers,
        word_diff_regex: format.word_diff_regex.as_deref(),
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

struct LinePrefixWriter<'a> {
    inner: &'a mut dyn Write,
    prefix: &'a [u8],
    at_line_start: bool,
}

impl<'a> LinePrefixWriter<'a> {
    fn new(inner: &'a mut dyn Write, prefix: &'a [u8]) -> Self {
        Self {
            inner,
            prefix,
            at_line_start: true,
        }
    }
}

impl Write for LinePrefixWriter<'_> {
    fn write(&mut self, mut buf: &[u8]) -> io::Result<usize> {
        let original_len = buf.len();
        while !buf.is_empty() {
            if self.at_line_start {
                self.inner.write_all(self.prefix)?;
                self.at_line_start = false;
            }
            let line_len = buf
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(buf.len(), |idx| idx + 1);
            self.inner.write_all(&buf[..line_len])?;
            self.at_line_start = buf[line_len - 1] == b'\n';
            buf = &buf[line_len..];
        }
        Ok(original_len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
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
    for entry in zmin_git_core::diff_trees(tree_cache, old_tree, new_tree)? {
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

fn index_entry_from_tree_diff_file(entry: &zmin_git_core::TreeDiffFileEntry) -> IndexEntry {
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
    pub(crate) ignore_blank_lines: bool,
    pub(crate) whitespace_mode: DiffWhitespaceMode,
    pub(crate) relative_prefix: Option<&'a [u8]>,
    pub(crate) text: bool,
    pub(crate) binary: bool,
    pub(crate) binary_threshold: u64,
    pub(crate) irreversible_delete: bool,
    pub(crate) submodule_format: SubmoduleDiffFormat,
    pub(crate) color: bool,
    pub(crate) emit_hunk_headers: bool,
    pub(crate) word_diff_regex: Option<&'a str>,
}

pub(crate) fn write_patch_entry<W: Write>(
    out: &mut W,
    context: &PatchWriteContext<'_>,
    entry: &zmin_git_core::IndexDiffEntry,
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
        let submodule_entry = zmin_git_core::IndexDiffEntry {
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
            old_content.len() as u64 > context.binary_threshold
                || new_content.len() as u64 > context.binary_threshold
                || is_binary_content(old_content)
                || is_binary_content(new_content)
        }
    };
    let visible_text_changes = if !binary
        && (context.whitespace_mode != DiffWhitespaceMode::None
            || !context.ignore_matching_lines.is_empty()
            || context.ignore_blank_lines)
    {
        Some({
            let _trace = phase_trace("format_patch.write_tree_diff.entry_visible_changes");
            diff_line_counts_with_options(
                old_content,
                new_content,
                context.whitespace_mode,
                &context.ignore_matching_lines,
                context.ignore_blank_lines,
            )
        })
    } else {
        None
    };
    if entry.status == IndexDiffStatus::Modified
        && let (Some(old_entry), Some(new_entry)) = (old_entry, new_entry)
        && old_entry.id == new_entry.id
        && old_entry.mode == new_entry.mode
        && old_content == new_content
    {
        return Ok(());
    }
    if entry.status == IndexDiffStatus::Modified
        && let Some((0, 0)) = visible_text_changes
    {
        return Ok(());
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
                context.binary && binary,
                Some(mode),
            )?;
        } else {
            write_patch_index_line(
                out,
                context.color,
                old_entry.map(|entry| &entry.id),
                new_entry.map(|entry| &entry.id),
                context.abbrev_len,
                context.binary && binary,
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
                word_diff_regex: context.word_diff_regex,
                color: context.color,
                unified_context: context.unified_context,
                inter_hunk_context: context.inter_hunk_context,
                output_indicator_new: context.output_indicator_new,
                output_indicator_old: context.output_indicator_old,
                output_indicator_context: context.output_indicator_context,
                ignore_matching_lines: context.ignore_matching_lines,
                ignore_blank_lines: context.ignore_blank_lines,
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
    if let Some((0, 0)) = visible_text_changes {
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
                word_diff_regex: context.word_diff_regex,
                color: context.color,
                unified_context: context.unified_context,
                inter_hunk_context: context.inter_hunk_context,
                output_indicator_new: context.output_indicator_new,
                output_indicator_old: context.output_indicator_old,
                output_indicator_context: context.output_indicator_context,
                ignore_matching_lines: context.ignore_matching_lines,
                ignore_blank_lines: context.ignore_blank_lines,
                whitespace_mode: context.whitespace_mode,
                emit_hunk_headers: context.emit_hunk_headers,
            },
        )
    }
}

pub(crate) fn write_submodule_diff_entry<W: Write>(
    out: &mut W,
    context: &PatchWriteContext<'_>,
    entry: &zmin_git_core::IndexDiffEntry,
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
    if entry.mode == IndexMode::Symlink {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return read_symlink_content(&path);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let _trace = phase_trace(
                    "format_patch.write_tree_diff.entry_content.worktree_or_index.fallback_index",
                );
                return read_index_entry_content(store, entry);
            }
            Err(error) => return Err(CliError::Io(error)),
            _ => {}
        }
    }
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
    word_diff_regex: Option<&'a str>,
    color: bool,
    unified_context: usize,
    inter_hunk_context: usize,
    output_indicator_new: Option<u8>,
    output_indicator_old: Option<u8>,
    output_indicator_context: Option<u8>,
    ignore_matching_lines: &'a [Regex],
    ignore_blank_lines: bool,
    whitespace_mode: DiffWhitespaceMode,
    emit_hunk_headers: bool,
}

impl Default for HunkFormatOptions<'static> {
    fn default() -> Self {
        Self {
            word_diff: WordDiffMode::None,
            word_diff_regex: None,
            color: false,
            unified_context: 3,
            inter_hunk_context: 0,
            output_indicator_new: Some(b'+'),
            output_indicator_old: Some(b'-'),
            output_indicator_context: Some(b' '),
            ignore_matching_lines: &[],
            ignore_blank_lines: false,
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
        diff_line_ops_for_path_with_whitespace(&old_lines, &new_lines, format.whitespace_mode, path)
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

        let mut hunk_old_start = old_line;
        let mut hunk_new_start = new_line;
        let mut hunk_ops = ops[start..end].to_vec();
        if path_uses_rust_structural_diff_heuristics(path) {
            normalize_terminal_inserted_attribute_hunk_in_place(&mut hunk_ops);
        }
        let trimmed_leading_context =
            trim_hunk_context_after_normalization_in_place(&mut hunk_ops, format.unified_context);
        hunk_old_start += trimmed_leading_context;
        hunk_new_start += trimmed_leading_context;
        let mut old_count = 0usize;
        let mut new_count = 0usize;

        for op in &hunk_ops {
            match op {
                DiffLineOp::Equal(_) => {
                    old_count += 1;
                    new_count += 1;
                }
                DiffLineOp::Delete(_) => old_count += 1,
                DiffLineOp::Insert(_) => new_count += 1,
            }
        }

        for op in &ops[start..end] {
            match op {
                DiffLineOp::Equal(_) => {
                    old_line += 1;
                    new_line += 1;
                }
                DiffLineOp::Delete(_) => {
                    old_line += 1;
                }
                DiffLineOp::Insert(_) => {
                    new_line += 1;
                }
            }
        }

        write_unified_hunk(
            out,
            &hunk_ops,
            &old_lines,
            path,
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
                    &[],
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
    hunk_ops: &[DiffLineOp<'_>],
    old_lines: &[&[u8]],
    path: &str,
    format: &HunkFormatOptions<'_>,
    header_cache: Option<&HunkHeaderCache>,
    hunk_old_start: usize,
    hunk_new_start: usize,
    hunk_old_count: usize,
    hunk_new_count: usize,
) -> Result<()> {
    if hunk_ignored_by_matching_lines(
        hunk_ops,
        0,
        hunk_ops.len(),
        format.ignore_matching_lines,
        format.ignore_blank_lines,
    ) {
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
            hunk_ops,
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
        let mut idx = 0usize;
        while idx < hunk_ops.len() {
            let op = &hunk_ops[idx];
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
                    let next_idx = write_word_diff_change_block(
                        out,
                        hunk_ops,
                        idx,
                        hunk_ops.len(),
                        format.word_diff,
                        format.word_diff_regex,
                    )?;
                    if next_idx != idx {
                        idx = next_idx;
                        continue;
                    }
                    write_word_diff_delete_line(
                        out,
                        line,
                        format.word_diff,
                        format.word_diff_regex,
                    )?
                }
                DiffLineOp::Delete(line) => write_diff_line(
                    out,
                    format.output_indicator_old,
                    line,
                    diff_line_color(format.color, DiffLineColor::Delete),
                )?,
                DiffLineOp::Insert(line) if format.word_diff != WordDiffMode::None => {
                    write_word_diff_insert_line(
                        out,
                        line,
                        format.word_diff,
                        format.word_diff_regex,
                    )?
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

fn normalize_terminal_inserted_attribute_hunk_in_place(ops: &mut Vec<DiffLineOp<'_>>) {
    if ops.len() < 2 {
        return;
    }
    let mut equal_start = ops.len();
    while equal_start > 0 && matches!(ops[equal_start - 1], DiffLineOp::Equal(_)) {
        equal_start -= 1;
    }
    if equal_start == ops.len()
        || !matches!(
            ops.get(equal_start),
            Some(DiffLineOp::Equal(line)) if is_direct_declaration_line_bytes(line)
        )
    {
        return;
    }
    let mut insert_start = equal_start;
    while insert_start > 0 && matches!(ops[insert_start - 1], DiffLineOp::Insert(_)) {
        insert_start -= 1;
    }
    if insert_start == equal_start {
        return;
    }
    let trailing_attr_len = inserted_attribute_suffix_len(&ops[insert_start..equal_start]);
    if trailing_attr_len == 0
        || !insert_run_contains_direct_declaration_before_suffix(
            &ops[insert_start..equal_start],
            trailing_attr_len,
        )
    {
        return;
    }
    for op in &mut ops[equal_start - trailing_attr_len..equal_start] {
        if let DiffLineOp::Insert(line) = *op {
            *op = DiffLineOp::Equal(line);
        }
    }
    if matches!(ops.last(), Some(DiffLineOp::Equal(line)) if is_blank_diff_line(line)) {
        ops.pop();
    }
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
    ignore_blank_lines: bool,
) -> bool {
    if ignore_matching_lines.is_empty() && !ignore_blank_lines {
        return false;
    }
    let mut saw_change = false;
    for op in &ops[start..end] {
        let line = match op {
            DiffLineOp::Equal(_) => continue,
            DiffLineOp::Delete(line) | DiffLineOp::Insert(line) => *line,
        };
        saw_change = true;
        if ignore_blank_lines && diff_ignore_blank_line(line) {
            continue;
        }
        if !ignore_matching_lines
            .iter()
            .any(|pattern| pattern.is_match(strip_trailing_lf(line)))
        {
            return false;
        }
    }
    saw_change
}

fn diff_ignore_blank_line(line: &[u8]) -> bool {
    strip_trailing_lf(line)
        .iter()
        .all(|byte| matches!(*byte, b' ' | b'\t' | b'\r'))
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
    regex: Option<&str>,
) -> Result<()> {
    let old_line = strip_trailing_lf(old);
    let new_line = strip_trailing_lf(new);
    let old_words = split_word_diff_tokens(old_line, regex)?;
    let new_words = split_word_diff_tokens(new_line, regex)?;
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
    regex: Option<&str>,
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
        write_word_diff_line(out, old_line, new_line, mode, regex)?;
    }
    for op in ops.iter().take(delete_end).skip(start + paired) {
        let DiffLineOp::Delete(line) = op else {
            return Err(CliError::Fatal {
                code: 128,
                message: "invalid word diff delete segment".into(),
            });
        };
        write_word_diff_delete_line(out, line, mode, regex)?;
    }
    for op in ops.iter().take(insert_end).skip(delete_end + paired) {
        let DiffLineOp::Insert(line) = op else {
            return Err(CliError::Fatal {
                code: 128,
                message: "invalid word diff insert segment".into(),
            });
        };
        write_word_diff_insert_line(out, line, mode, regex)?;
    }
    Ok(insert_end)
}

pub(crate) fn write_word_diff_delete_line<W: Write>(
    out: &mut W,
    line: &[u8],
    mode: WordDiffMode,
    regex: Option<&str>,
) -> Result<()> {
    match mode {
        WordDiffMode::Plain => {
            out.write_all(b"[-")?;
            write_word_diff_regex_line_body(out, strip_trailing_lf(line), regex)?;
            out.write_all(b"-]")?;
            writeln!(out)?;
        }
        WordDiffMode::Color => {
            out.write_all(b"\x1b[31m")?;
            write_word_diff_regex_line_body(out, strip_trailing_lf(line), regex)?;
            out.write_all(b"\x1b[m")?;
            writeln!(out)?;
        }
        WordDiffMode::Porcelain => {
            write_word_diff_porcelain_segment_with_regex(
                out,
                b'-',
                strip_trailing_lf(line),
                regex,
            )?;
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
    regex: Option<&str>,
) -> Result<()> {
    match mode {
        WordDiffMode::Plain => {
            out.write_all(b"{+")?;
            write_word_diff_regex_line_body(out, strip_trailing_lf(line), regex)?;
            out.write_all(b"+}")?;
            writeln!(out)?;
        }
        WordDiffMode::Color => {
            out.write_all(b"\x1b[32m")?;
            write_word_diff_regex_line_body(out, strip_trailing_lf(line), regex)?;
            out.write_all(b"\x1b[m")?;
            writeln!(out)?;
        }
        WordDiffMode::Porcelain => {
            write_word_diff_porcelain_segment_with_regex(
                out,
                b'+',
                strip_trailing_lf(line),
                regex,
            )?;
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

pub(crate) fn split_word_diff_tokens<'a>(
    line: &'a [u8],
    regex: Option<&str>,
) -> Result<Vec<&'a [u8]>> {
    if line.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(pattern) = regex {
        return split_word_diff_tokens_with_regex(line, pattern);
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
    Ok(tokens)
}

fn validate_word_diff_regex(regex: Option<&str>) -> Result<()> {
    if let Some(pattern) = regex {
        Regex::new(pattern).map_err(|_| CliError::Fatal {
            code: 128,
            message: format!("invalid regular expression: {pattern}"),
        })?;
    }
    Ok(())
}

fn split_word_diff_tokens_with_regex<'a>(line: &'a [u8], pattern: &str) -> Result<Vec<&'a [u8]>> {
    let regex = Regex::new(pattern).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid regular expression: {pattern}"),
    })?;
    let mut tokens = Vec::new();
    let mut last = 0usize;
    for matched in regex.find_iter(line) {
        if matched.start() > last {
            tokens.push(&line[last..matched.start()]);
        }
        tokens.push(&line[matched.start()..matched.end()]);
        last = matched.end();
    }
    if last < line.len() {
        tokens.push(&line[last..]);
    }
    if tokens.is_empty() {
        tokens.push(line);
    }
    Ok(tokens)
}

fn write_word_diff_regex_line_body<W: Write>(
    out: &mut W,
    line: &[u8],
    regex: Option<&str>,
) -> Result<()> {
    for token in split_word_diff_tokens(line, regex)? {
        out.write_all(token)?;
    }
    Ok(())
}

fn write_word_diff_porcelain_segment_with_regex<W: Write>(
    out: &mut W,
    prefix: u8,
    line: &[u8],
    regex: Option<&str>,
) -> Result<()> {
    for token in split_word_diff_tokens(line, regex)? {
        write_word_diff_porcelain_segment(out, prefix, token)?;
    }
    Ok(())
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
    _hunk_ops: &[DiffLineOp<'_>],
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
    if path.ends_with(".tsv") || path.ends_with(".csv") {
        return None;
    }
    if path.ends_with(".md") {
        return markdown_hunk_header(old_lines, old_start);
    }
    if path.ends_with(".js")
        || path.ends_with(".mjs")
        || path.ends_with(".cjs")
        || path.ends_with(".ts")
        || path.ends_with(".mts")
        || path.ends_with(".cts")
        || path.ends_with(".jsx")
        || path.ends_with(".tsx")
        || path.ends_with(".vue")
    {
        if let Some(cache) = header_cache {
            return cache.get(old_start);
        }
        return js_hunk_header(old_lines, old_start);
    }
    if path.ends_with(".rs") {
        return rust_hunk_header(old_lines, old_start);
    }
    if path.ends_with(".yml") || path.ends_with(".yaml") {
        if let Some(cache) = header_cache {
            return cache.get(old_start);
        }
        return yaml_hunk_header(old_lines, old_start);
    }
    if path.ends_with(".sh") {
        return shell_hunk_header(old_lines, old_start);
    }
    if path == "Dockerfile" || path.ends_with("/Dockerfile") {
        return hunk_line_header(old_lines, old_start.saturating_sub(1));
    }
    let _ = old_count;
    default_hunk_header(old_lines, old_start)
}

#[derive(Debug)]
struct HunkHeaderCache {
    line_to_last_header: Vec<usize>,
    cached_headers: Vec<Option<String>>,
}

impl HunkHeaderCache {
    fn get(&self, old_start: usize) -> Option<String> {
        let target_start = old_start.saturating_sub(2);
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
        || path.ends_with(".js")
        || path.ends_with(".mjs")
        || path.ends_with(".cjs")
        || path.ends_with(".ts")
        || path.ends_with(".mts")
        || path.ends_with(".cts")
        || path.ends_with(".jsx")
        || path.ends_with(".tsx")
        || path.ends_with(".vue")
        || path.ends_with(".rs")
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

    if path.ends_with(".js")
        || path.ends_with(".mjs")
        || path.ends_with(".cjs")
        || path.ends_with(".ts")
        || path.ends_with(".mts")
        || path.ends_with(".cts")
        || path.ends_with(".jsx")
        || path.ends_with(".tsx")
        || path.ends_with(".vue")
    {
        return Some(build_hunk_header_cache_kind(
            old_lines,
            cache_js_header_line,
        ));
    }

    if path.ends_with(".rs") {
        return Some(build_hunk_header_cache_kind(
            old_lines,
            cache_rust_header_line,
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
            if let Some(line) = parser(old_lines[idx]) {
                cached_headers[idx] = Some(line);
                last = idx;
            }
            line_to_last_header[idx] = last;
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

fn cache_js_header_line(line: &[u8]) -> Option<String> {
    let candidate = js_hunk_header_line(line)?;
    js_hunk_header_candidate(&candidate).then_some(candidate)
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

fn cache_rust_header_line(line: &[u8]) -> Option<String> {
    let candidate = rust_hunk_header_line(line)?;
    rust_hunk_header_candidate(&candidate).then_some(candidate)
}

pub(crate) fn markdown_hunk_header(old_lines: &[&[u8]], old_start: usize) -> Option<String> {
    for idx in (0..old_start.saturating_sub(1)).rev() {
        let line = *old_lines.get(idx)?;
        let text = String::from_utf8_lossy(line);
        let trimmed_end = text.trim_end_matches(['\r', '\n']);
        if trimmed_end.starts_with([' ', '\t']) {
            continue;
        }
        let Some(candidate) = clean_hunk_header_line(trimmed_end.as_bytes()) else {
            continue;
        };
        if candidate == "---"
            || candidate.starts_with("**")
            || candidate.starts_with('#')
            || candidate.starts_with('|')
            || candidate.starts_with("- ")
            || candidate.starts_with("* ")
            || candidate.starts_with('[')
            || candidate.starts_with('`')
        {
            continue;
        }
        return Some(candidate);
    }
    None
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

pub(crate) fn js_hunk_header(old_lines: &[&[u8]], old_start: usize) -> Option<String> {
    for idx in (0..old_start.saturating_sub(1)).rev() {
        let Some(candidate) = js_hunk_header_line(old_lines.get(idx)?) else {
            continue;
        };
        if js_hunk_header_candidate(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn js_hunk_header_line(line: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(line);
    let trimmed_end = text.trim_end_matches(['\r', '\n']);
    if trimmed_end.starts_with([' ', '\t']) {
        return None;
    }
    clean_hunk_header_line(trimmed_end.as_bytes())
}

fn js_hunk_header_candidate(candidate: &str) -> bool {
    let candidate = candidate.trim_start();
    if candidate.is_empty()
        || candidate.starts_with("//")
        || candidate.starts_with('*')
        || candidate == "{"
        || candidate == "}"
    {
        return false;
    }
    candidate.starts_with("function ")
        || candidate.starts_with("async function ")
        || candidate.starts_with("const ")
        || candidate.starts_with("let ")
        || candidate.starts_with("var ")
        || candidate.starts_with("class ")
        || candidate.starts_with("interface ")
        || candidate.starts_with("type ")
        || candidate.starts_with("enum ")
        || candidate.starts_with("export ")
        || candidate.starts_with("for ")
        || candidate.starts_with("for(")
        || candidate.starts_with("while ")
        || candidate.starts_with("if ")
        || candidate.contains("=>")
}

pub(crate) fn rust_hunk_header(old_lines: &[&[u8]], old_start: usize) -> Option<String> {
    for idx in (0..old_start.saturating_sub(1)).rev() {
        let line = *old_lines.get(idx)?;
        let Some(candidate) = rust_hunk_header_line(line) else {
            continue;
        };
        if rust_hunk_header_candidate(&candidate) {
            return Some(candidate);
        }
        if rust_top_level_context_header_candidate(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn rust_hunk_header_line(line: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(line);
    let trimmed_end = text.trim_end_matches(['\r', '\n']);
    if trimmed_end.starts_with([' ', '\t']) {
        return None;
    }
    clean_hunk_header_line(trimmed_end.as_bytes())
}

fn rust_hunk_header_candidate(candidate: &str) -> bool {
    if candidate.starts_with('#') || candidate == "{" || candidate == "}" {
        return false;
    }
    let Some(stripped) = rust_hunk_header_strip_visibility(candidate) else {
        return false;
    };
    let stripped = rust_hunk_header_strip_qualifiers(stripped);
    stripped.starts_with("fn ")
        || stripped.starts_with("struct ")
        || stripped.starts_with("enum ")
        || stripped.starts_with("union ")
        || stripped.starts_with("trait ")
        || stripped.starts_with("impl")
        || stripped.starts_with("mod ")
        || stripped.starts_with("type ")
        || stripped.starts_with("const ")
        || stripped.starts_with("static ")
        || stripped.starts_with("macro_rules!")
}

fn rust_top_level_context_header_candidate(candidate: &str) -> bool {
    if candidate.is_empty() || candidate == "{" || candidate == "}" || candidate.starts_with('#') {
        return false;
    }
    candidate
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphanumeric())
}

fn rust_hunk_header_strip_visibility(candidate: &str) -> Option<&str> {
    let candidate = candidate.trim_start();
    if let Some(rest) = candidate.strip_prefix("pub(crate) ") {
        return Some(rest);
    }
    if let Some(rest) = candidate.strip_prefix("pub(self) ") {
        return Some(rest);
    }
    if let Some(rest) = candidate.strip_prefix("pub(super) ") {
        return Some(rest);
    }
    if let Some(rest) = candidate.strip_prefix("pub(in ") {
        return rest.split_once(')').map(|(_, rest)| rest.trim_start());
    }
    if let Some(rest) = candidate.strip_prefix("pub ") {
        return Some(rest);
    }
    Some(candidate)
}

fn rust_hunk_header_strip_qualifiers(candidate: &str) -> &str {
    let mut current = candidate.trim_start();
    loop {
        let next = current
            .strip_prefix("async ")
            .or_else(|| current.strip_prefix("unsafe "))
            .or_else(|| current.strip_prefix("extern \"C\" "))
            .or_else(|| current.strip_prefix("extern \"Rust\" "))
            .or_else(|| current.strip_prefix("extern "))
            .or_else(|| current.strip_prefix("default "));
        let Some(next) = next else {
            break;
        };
        current = next.trim_start();
    }
    current
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

fn default_hunk_header(old_lines: &[&[u8]], old_start: usize) -> Option<String> {
    for idx in (0..old_start.saturating_sub(1)).rev() {
        let Some(candidate) = default_hunk_header_line(*old_lines.get(idx)?) else {
            continue;
        };
        return Some(candidate);
    }
    None
}

fn default_hunk_header_line(line: &[u8]) -> Option<String> {
    if line.is_empty() || line.contains(&0) {
        return None;
    }
    let first = *line.first()?;
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
        return None;
    }
    let line = String::from_utf8_lossy(line);
    let line = line.trim_end_matches(char::is_whitespace);
    if line.is_empty() {
        None
    } else {
        Some(truncate_hunk_header_line(line))
    }
}

fn shell_hunk_header(old_lines: &[&[u8]], old_start: usize) -> Option<String> {
    for idx in (0..old_start.saturating_sub(1)).rev() {
        let line = String::from_utf8_lossy(old_lines.get(idx)?);
        let line = line.trim_end_matches(['\r', '\n']);
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let candidate = line.trim();
        if candidate.is_empty()
            || candidate == "{"
            || candidate == "}"
            || candidate.starts_with('#')
            || candidate.starts_with('"')
            || candidate.starts_with('\'')
        {
            continue;
        }
        if candidate
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return Some(truncate_hunk_header_line(candidate));
        }
    }
    None
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

fn path_uses_rust_structural_diff_heuristics(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        == Some("rs")
}

fn normalize_diff_ops_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
    apply_structural_heuristics: bool,
) {
    if !apply_structural_heuristics {
        return;
    }
    normalize_diff_change_run_structural_common_prefix_to_equal_in_place(ops);
    normalize_diff_change_run_to_late_common_slice_in_place(ops);
    normalize_diff_equal_run_prefix_to_later_insert_duplicate_in_place(ops);
    normalize_diff_equal_run_to_later_insert_duplicate_in_place(ops);
    normalize_diff_declaration_alignment_in_place(ops);
    normalize_diff_equal_run_suffix_to_insert_prefix_duplicate_in_place(ops);
    normalize_diff_epilogue_ok_declaration_sandwich_in_place(ops);
    normalize_diff_inserted_structural_suffix_before_declaration_in_place(ops);
    normalize_diff_equal_epilogue_before_inserted_declaration_in_place(ops);
    normalize_diff_equal_epilogue_before_related_equal_declaration_in_place(ops);
    normalize_diff_insert_prefix_structural_duplicate_to_following_equal_in_place(ops);
    normalize_diff_inserted_epilogue_prefix_rebind_from_following_equal_in_place(ops);
    normalize_diff_equal_epilogue_after_inserted_declaration_with_equal_context_in_place(ops);
    normalize_diff_blank_alignment_in_place(ops);
}

fn diff_line_ops_with_whitespace_and_heuristics<'a>(
    old: &[&'a [u8]],
    new: &[&'a [u8]],
    whitespace_mode: DiffWhitespaceMode,
    apply_structural_heuristics: bool,
) -> Vec<DiffLineOp<'a>> {
    let mut prefix_len = 0usize;
    while prefix_len < old.len()
        && prefix_len < new.len()
        && diff_lines_equal(old[prefix_len], new[prefix_len], whitespace_mode)
    {
        prefix_len += 1;
    }

    let mut suffix_len = 0usize;
    let mut suffix_saw_block_boundary = false;
    while prefix_len + suffix_len < old.len() && prefix_len + suffix_len < new.len() {
        let old_line = old[old.len() - 1 - suffix_len];
        let new_line = new[new.len() - 1 - suffix_len];
        if !diff_lines_equal(old_line, new_line, whitespace_mode) {
            break;
        }
        if suffix_saw_block_boundary && is_function_epilogue_line(old_line) {
            break;
        }
        suffix_len += 1;
        if is_insert_block_boundary_line_bytes(old_line) {
            suffix_saw_block_boundary = true;
        }
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
    normalize_diff_ops_in_place(&mut ops, apply_structural_heuristics);
    ops
}

pub(crate) fn diff_line_ops_with_whitespace<'a>(
    old: &[&'a [u8]],
    new: &[&'a [u8]],
    whitespace_mode: DiffWhitespaceMode,
) -> Vec<DiffLineOp<'a>> {
    diff_line_ops_with_whitespace_and_heuristics(old, new, whitespace_mode, true)
}

fn diff_line_ops_for_path_with_whitespace<'a>(
    old: &[&'a [u8]],
    new: &[&'a [u8]],
    whitespace_mode: DiffWhitespaceMode,
    path: &str,
) -> Vec<DiffLineOp<'a>> {
    let mut ops = diff_line_ops_with_whitespace_and_heuristics(
        old,
        new,
        whitespace_mode,
        path_uses_rust_structural_diff_heuristics(path),
    );
    if !path_uses_rust_structural_diff_heuristics(path) {
        normalize_diff_blank_alignment_in_place(&mut ops);
    }
    normalize_diff_deleted_line_to_matching_insert_tail_in_place(&mut ops);
    ops
}

fn normalize_diff_deleted_line_to_matching_insert_tail_in_place<'a>(ops: &mut Vec<DiffLineOp<'a>>) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let DiffLineOp::Delete(deleted) = ops[idx] else {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        };
        let insert_start = idx + 1;
        let mut insert_end = insert_start;
        while insert_end < ops.len() && matches!(ops[insert_end], DiffLineOp::Insert(_)) {
            insert_end += 1;
        }
        let matching_tail = insert_end
            .checked_sub(1)
            .filter(|tail| *tail >= insert_start)
            .is_some_and(
                |tail| matches!(ops[tail], DiffLineOp::Insert(inserted) if inserted == deleted),
            );
        if !matching_tail {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }
        rewritten.extend_from_slice(&ops[insert_start..insert_end - 1]);
        rewritten.push(DiffLineOp::Equal(deleted));
        idx = insert_end;
    }
    *ops = rewritten;
}

const DIFF_MYERS_CELL_THRESHOLD: usize = 16_384;
const DIFF_MYERS_MIN_LINES: usize = 32;
type LineLcsLen = u16;
const FULL_EQUAL_REBIND_MAX_INSERT_PREFIX: usize = 64;
const PARTIAL_EQUAL_REBIND_MAX_INSERT_PREFIX: usize = 32;
const OK_SUFFIX_REBIND_MAX_INSERT_PREFIX: usize = 512;

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
    ops
}

fn assert_line_lcs_len_fits(old_len: usize, new_len: usize) {
    let max_len = old_len.max(new_len);
    assert!(
        LineLcsLen::try_from(max_len).is_ok(),
        "line diff input is too large to store compact LCS lengths"
    );
}

pub(crate) fn normalize_diff_change_run_structural_common_prefix_to_equal_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let delete_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Delete(_)) {
            idx += 1;
        }
        let delete_len = idx - delete_start;
        if delete_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            rewritten.extend_from_slice(&ops[delete_start..delete_start + delete_len]);
            continue;
        }

        let prefix_len = common_structural_change_prefix_len(
            &ops[delete_start..delete_start + delete_len],
            &ops[insert_start..insert_start + insert_len],
        );
        if prefix_len > 0
            && change_runs_eventually_diverge_after_prefix(
                &ops[delete_start..delete_start + delete_len],
                &ops[insert_start..insert_start + insert_len],
                prefix_len,
            )
        {
            rewritten.extend(
                ops[delete_start..delete_start + prefix_len]
                    .iter()
                    .copied()
                    .map(|op| match op {
                        DiffLineOp::Delete(line) => DiffLineOp::Equal(line),
                        _ => op,
                    }),
            );
            rewritten.extend_from_slice(&ops[delete_start + prefix_len..delete_start + delete_len]);
            rewritten.extend_from_slice(&ops[insert_start + prefix_len..insert_start + insert_len]);
            continue;
        }

        rewritten.extend_from_slice(&ops[delete_start..delete_start + delete_len]);
        rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
    }
    *ops = rewritten;
}

pub(crate) fn normalize_diff_change_run_to_late_common_slice_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let delete_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Delete(_)) {
            idx += 1;
        }
        let delete_len = idx - delete_start;
        if delete_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            rewritten.extend_from_slice(&ops[delete_start..delete_start + delete_len]);
            continue;
        }

        let delete_run = &ops[delete_start..delete_start + delete_len];
        let insert_run = &ops[insert_start..insert_start + insert_len];
        if insert_run_contains_direct_declaration(insert_run)
            || matches!(
                delete_run.first(),
                Some(DiffLineOp::Delete(line)) if is_insert_block_boundary_line_bytes(line)
            )
            || matches!(
                insert_run.first(),
                Some(DiffLineOp::Insert(line)) if is_insert_block_boundary_line_bytes(line)
            )
        {
            rewritten.extend_from_slice(delete_run);
            rewritten.extend_from_slice(insert_run);
            continue;
        }

        if let Some((matched_len, insert_match_start)) =
            longest_delete_suffix_inside_insert_run(delete_run, insert_run)
        {
            let delete_keep_len = delete_len - matched_len;
            let insert_prefix_len = insert_match_start;
            let insert_suffix_start = insert_match_start + matched_len;

            rewritten.extend_from_slice(&ops[delete_start..delete_start + delete_keep_len]);
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_prefix_len]);
            rewritten.extend(
                ops[delete_start + delete_keep_len..delete_start + delete_len]
                    .iter()
                    .copied()
                    .map(|op| match op {
                        DiffLineOp::Delete(line) => DiffLineOp::Equal(line),
                        _ => op,
                    }),
            );
            rewritten.extend_from_slice(
                &ops[insert_start + insert_suffix_start..insert_start + insert_len],
            );
        } else {
            rewritten.extend_from_slice(&ops[delete_start..delete_start + delete_len]);
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
        }
    }
    *ops = rewritten;
}

fn longest_delete_suffix_inside_insert_run(
    delete_run: &[DiffLineOp<'_>],
    insert_run: &[DiffLineOp<'_>],
) -> Option<(usize, usize)> {
    let max_match_len = delete_run.len().min(insert_run.len());
    for matched_len in (1..=max_match_len).rev() {
        let delete_suffix = &delete_run[delete_run.len() - matched_len..];
        for insert_match_start in (0..=insert_run.len() - matched_len).rev() {
            let insert_slice = &insert_run[insert_match_start..insert_match_start + matched_len];
            let is_match =
                delete_suffix
                    .iter()
                    .zip(insert_slice)
                    .all(|(deleted, inserted)| match (*deleted, *inserted) {
                        (DiffLineOp::Delete(deleted_line), DiffLineOp::Insert(inserted_line)) => {
                            deleted_line == inserted_line
                        }
                        _ => false,
                    });
            if is_match {
                return Some((matched_len, insert_match_start));
            }
        }
    }
    None
}

fn common_structural_change_prefix_len(
    delete_run: &[DiffLineOp<'_>],
    insert_run: &[DiffLineOp<'_>],
) -> usize {
    delete_run
        .iter()
        .zip(insert_run)
        .take_while(|(deleted, inserted)| match (**deleted, **inserted) {
            (DiffLineOp::Delete(deleted_line), DiffLineOp::Insert(inserted_line)) => {
                deleted_line == inserted_line && is_structural_prefix_line(deleted_line)
            }
            _ => false,
        })
        .count()
}

fn change_runs_eventually_diverge_after_prefix(
    delete_run: &[DiffLineOp<'_>],
    insert_run: &[DiffLineOp<'_>],
    prefix_len: usize,
) -> bool {
    for (deleted, inserted) in delete_run[prefix_len..]
        .iter()
        .zip(&insert_run[prefix_len..])
    {
        match (*deleted, *inserted) {
            (DiffLineOp::Delete(deleted_line), DiffLineOp::Insert(inserted_line)) => {
                if deleted_line != inserted_line {
                    return true;
                }
            }
            _ => return true,
        }
    }
    delete_run.len() != insert_run.len()
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

fn trim_hunk_context_after_normalization_in_place(
    ops: &mut Vec<DiffLineOp<'_>>,
    unified_context: usize,
) -> usize {
    let Some(first_change_idx) = ops.iter().position(DiffLineOp::is_change) else {
        return 0;
    };
    let Some(last_change_idx) = ops.iter().rposition(DiffLineOp::is_change) else {
        return 0;
    };

    let leading_trim = first_change_idx.saturating_sub(unified_context);
    let trailing_equal_count = ops.len().saturating_sub(last_change_idx + 1);
    let trailing_trim = trailing_equal_count.saturating_sub(unified_context);

    if leading_trim == 0 && trailing_trim == 0 {
        return 0;
    }

    let end = ops.len() - trailing_trim;
    ops.drain(end..);
    ops.drain(..leading_trim);
    leading_trim
}

pub(crate) fn normalize_diff_equal_run_prefix_to_later_insert_duplicate_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let equal_len = idx - equal_start;
        if equal_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            continue;
        }

        let matched_prefix_len = longest_equal_run_prefix_matching_insert_suffix(
            &ops[equal_start..equal_start + equal_len],
            &ops[insert_start..insert_start + insert_len],
        )
        .filter(|matched_prefix_len| {
            *matched_prefix_len < equal_len
                && equal_run_prefix_starts_with_opener_line(
                    &ops[equal_start..equal_start + *matched_prefix_len],
                )
        });

        if let Some(matched_prefix_len) = matched_prefix_len {
            rewritten.extend(
                ops[equal_start..equal_start + matched_prefix_len]
                    .iter()
                    .copied()
                    .map(|op| match op {
                        DiffLineOp::Equal(line) => DiffLineOp::Insert(line),
                        _ => op,
                    }),
            );
            rewritten.extend_from_slice(
                &ops[insert_start..insert_start + insert_len - matched_prefix_len],
            );
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            continue;
        }

        rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
        rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
    }
    *ops = rewritten;
}

pub(crate) fn normalize_diff_equal_run_to_later_insert_duplicate_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let prev_is_change = rewritten.last().is_some_and(DiffLineOp::is_change);
        let equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let equal_len = idx - equal_start;
        if equal_len > 0 {
            let insert_start = idx;
            while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
                idx += 1;
            }
            let insert_len = idx - insert_start;
            let max_match_len = equal_len.min(insert_len);
            let mut rebound_suffix_len = None;
            for matched_len in if prev_is_change {
                EitherSuffixLens::Descending(1..=max_match_len)
            } else {
                EitherSuffixLens::Ascending(1..=max_match_len)
            } {
                if !insert_run_suffix_matches_equal_run(
                    &ops[equal_start + equal_len - matched_len..equal_start + equal_len],
                    &ops[insert_start..insert_start + insert_len],
                ) {
                    continue;
                }
                let insert_prefix_len = insert_len.saturating_sub(matched_len);
                let allowed = if matched_len == equal_len {
                    equal_run_starts_with_opener_line(&ops[equal_start..equal_start + equal_len])
                        && insert_prefix_len <= FULL_EQUAL_REBIND_MAX_INSERT_PREFIX
                } else {
                    equal_run_suffix_is_not_only_function_epilogue(
                        &ops[equal_start + equal_len - matched_len..equal_start + equal_len],
                    ) && equal_run_suffix_is_preceded_by_epilogue_line(
                        &ops[equal_start..equal_start + equal_len],
                        matched_len,
                    ) && equal_run_suffix_starts_with_rebindable_prefix_line(
                        &ops[equal_start + equal_len - matched_len..equal_start + equal_len],
                    ) && insert_prefix_len
                        <= if equal_run_suffix_starts_with_ok_line(
                            &ops[equal_start + equal_len - matched_len..equal_start + equal_len],
                        ) {
                            OK_SUFFIX_REBIND_MAX_INSERT_PREFIX
                        } else {
                            PARTIAL_EQUAL_REBIND_MAX_INSERT_PREFIX
                        }
                };
                if allowed {
                    rebound_suffix_len = Some(matched_len);
                    break;
                }
            }
            if let Some(rebound_suffix_len) = rebound_suffix_len.filter(|_| {
                ops.get(idx)
                    .is_some_and(|op| matches!(op, DiffLineOp::Equal(_)))
            }) {
                let equal_prefix_len = equal_len - rebound_suffix_len;
                rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_prefix_len]);
                rewritten.extend(
                    ops[equal_start + equal_prefix_len..equal_start + equal_len]
                        .iter()
                        .copied()
                        .map(|op| match op {
                            DiffLineOp::Equal(line) => DiffLineOp::Insert(line),
                            _ => op,
                        }),
                );
                rewritten.extend_from_slice(
                    &ops[insert_start..insert_start + insert_len - rebound_suffix_len],
                );
                rewritten.extend_from_slice(
                    &ops[equal_start + equal_prefix_len..equal_start + equal_len],
                );
                continue;
            }
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            continue;
        }
        if equal_len > 0 {
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            continue;
        }
        rewritten.push(ops[idx]);
        idx += 1;
    }
    *ops = rewritten;
}

pub(crate) fn normalize_diff_equal_run_suffix_to_insert_prefix_duplicate_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let equal_len = idx - equal_start;
        if equal_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            continue;
        }

        if insert_run_starts_with_attribute(&ops[insert_start..insert_start + insert_len])
            && insert_run_contains_direct_declaration(&ops[insert_start..insert_start + insert_len])
        {
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            continue;
        }

        let matched_suffix_len = longest_equal_run_suffix_matching_insert_prefix(
            &ops[equal_start..equal_start + equal_len],
            &ops[insert_start..insert_start + insert_len],
        );
        if let Some(matched_suffix_len) = matched_suffix_len
            .filter(|matched| *matched < insert_len)
            .filter(|matched| {
                !equal_run_starts_with_insert_block_boundary(
                    &ops[equal_start..equal_start + equal_len],
                ) && insert_remainder_starts_with_declaration_or_attribute(
                    &ops[insert_start + matched..insert_start + insert_len],
                )
            })
        {
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            rewritten.extend_from_slice(
                &ops[insert_start + matched_suffix_len..insert_start + insert_len],
            );
            continue;
        }

        rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
        rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
    }
    *ops = rewritten;
}

fn longest_equal_run_prefix_matching_insert_suffix(
    equal_run: &[DiffLineOp<'_>],
    insert_run: &[DiffLineOp<'_>],
) -> Option<usize> {
    let max_match_len = equal_run.len().min(insert_run.len());
    let mut matched = None;
    for matched_len in 1..=max_match_len {
        if equal_run_prefix_matches_insert_run_suffix(&equal_run[..matched_len], insert_run) {
            matched = Some(matched_len);
        }
    }
    matched
}

fn longest_equal_run_suffix_matching_insert_prefix(
    equal_run: &[DiffLineOp<'_>],
    insert_run: &[DiffLineOp<'_>],
) -> Option<usize> {
    let max_match_len = equal_run.len().min(insert_run.len());
    let mut matched = None;
    for matched_len in 1..=max_match_len {
        if equal_run_suffix_matches_insert_prefix(
            &equal_run[equal_run.len() - matched_len..],
            &insert_run[..matched_len],
        ) {
            matched = Some(matched_len);
        }
    }
    matched
}

fn equal_run_prefix_matches_insert_run_suffix(
    equal_prefix: &[DiffLineOp<'_>],
    insert_run: &[DiffLineOp<'_>],
) -> bool {
    equal_prefix
        .iter()
        .zip(&insert_run[insert_run.len() - equal_prefix.len()..])
        .all(|(equal, inserted)| match (*equal, *inserted) {
            (DiffLineOp::Equal(equal_line), DiffLineOp::Insert(inserted_line)) => {
                equal_line == inserted_line
            }
            _ => false,
        })
}

fn equal_run_suffix_matches_insert_prefix(
    equal_suffix: &[DiffLineOp<'_>],
    insert_prefix: &[DiffLineOp<'_>],
) -> bool {
    equal_suffix
        .iter()
        .zip(insert_prefix)
        .all(|(equal, inserted)| match (*equal, *inserted) {
            (DiffLineOp::Equal(equal_line), DiffLineOp::Insert(inserted_line)) => {
                equal_line == inserted_line
            }
            _ => false,
        })
}

fn insert_remainder_starts_with_declaration_or_attribute(insert_run: &[DiffLineOp<'_>]) -> bool {
    match insert_run.first().copied() {
        Some(DiffLineOp::Insert(line)) => is_insert_block_boundary_line_bytes(line),
        _ => false,
    }
}

fn normalize_diff_equal_attribute_suffix_before_inserted_declaration_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let left_equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let left_equal_len = idx - left_equal_start;
        if left_equal_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            continue;
        }

        let right_equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let right_equal_len = idx - right_equal_start;
        if right_equal_len == 0 {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            continue;
        }
        if !insert_run_contains_direct_declaration(&ops[insert_start..insert_start + insert_len])
            || !matches!(
                ops[right_equal_start],
                DiffLineOp::Equal(line) if is_direct_declaration_line_bytes(line)
            )
        {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            rewritten
                .extend_from_slice(&ops[right_equal_start..right_equal_start + right_equal_len]);
            continue;
        }

        let attribute_suffix_len = left_equal_run_attribute_suffix_len(
            &ops[left_equal_start..left_equal_start + left_equal_len],
        );
        if attribute_suffix_len == 0 {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            rewritten
                .extend_from_slice(&ops[right_equal_start..right_equal_start + right_equal_len]);
            continue;
        }

        rewritten.extend_from_slice(
            &ops[left_equal_start..left_equal_start + left_equal_len - attribute_suffix_len],
        );
        rewritten.extend(
            ops[left_equal_start + left_equal_len - attribute_suffix_len
                ..left_equal_start + left_equal_len]
                .iter()
                .copied()
                .map(|op| match op {
                    DiffLineOp::Equal(line) => DiffLineOp::Insert(line),
                    _ => op,
                }),
        );
        rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
        rewritten.extend_from_slice(&ops[right_equal_start..right_equal_start + right_equal_len]);
    }
    *ops = rewritten;
}

fn left_equal_run_attribute_suffix_len(equal_run: &[DiffLineOp<'_>]) -> usize {
    let mut suffix_len = 0usize;
    for op in equal_run.iter().rev() {
        match *op {
            DiffLineOp::Equal(line) if is_attribute_line_bytes(line) => suffix_len += 1,
            _ => break,
        }
    }
    suffix_len
}

fn normalize_diff_inserted_declaration_with_shared_body_prefix_against_following_declaration_in_place<
    'a,
>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let left_equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let left_equal_len = idx - left_equal_start;
        if left_equal_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let first_insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let first_insert_len = idx - first_insert_start;
        if first_insert_len == 0 {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            continue;
        }

        let delete_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Delete(_)) {
            idx += 1;
        }
        let delete_len = idx - delete_start;
        if delete_len == 0 {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            rewritten
                .extend_from_slice(&ops[first_insert_start..first_insert_start + first_insert_len]);
            continue;
        }

        let left_attr_suffix_len = left_equal_run_attribute_suffix_len(
            &ops[left_equal_start..left_equal_start + left_equal_len],
        );
        let Some(deleted_decl_line) =
            first_deleted_direct_declaration_line(&ops[delete_start..delete_start + delete_len])
        else {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            rewritten
                .extend_from_slice(&ops[first_insert_start..first_insert_start + first_insert_len]);
            rewritten.extend_from_slice(&ops[delete_start..delete_start + delete_len]);
            rewritten.extend_from_slice(&ops[idx..]);
            continue;
        };
        if left_attr_suffix_len == 0
            || !insert_run_starts_with_direct_declaration(
                &ops[first_insert_start..first_insert_start + first_insert_len],
            )
        {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            rewritten
                .extend_from_slice(&ops[first_insert_start..first_insert_start + first_insert_len]);
            rewritten.extend_from_slice(&ops[delete_start..delete_start + delete_len]);
            rewritten.extend_from_slice(&ops[idx..]);
            continue;
        }

        let left_attr_suffix = &ops[left_equal_start + left_equal_len - left_attr_suffix_len
            ..left_equal_start + left_equal_len];
        let search_start = idx;
        let Some((matched_insert_start, matched_insert_end, matched_attr_start)) =
            find_later_insert_run_with_matching_attribute_and_decl(
                ops,
                search_start,
                left_attr_suffix,
                deleted_decl_line,
            )
        else {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            rewritten
                .extend_from_slice(&ops[first_insert_start..first_insert_start + first_insert_len]);
            rewritten.extend_from_slice(&ops[delete_start..delete_start + delete_len]);
            rewritten.extend_from_slice(&ops[search_start..]);
            continue;
        };

        rewritten.extend_from_slice(
            &ops[left_equal_start..left_equal_start + left_equal_len - left_attr_suffix_len],
        );
        rewritten.extend(left_attr_suffix.iter().copied().map(|op| match op {
            DiffLineOp::Equal(line) => DiffLineOp::Insert(line),
            _ => op,
        }));
        rewritten
            .extend_from_slice(&ops[first_insert_start..first_insert_start + first_insert_len]);
        rewritten.extend(ops[search_start..matched_insert_start].iter().copied().map(
            |op| match op {
                DiffLineOp::Equal(line) => DiffLineOp::Insert(line),
                DiffLineOp::Insert(line) => DiffLineOp::Insert(line),
                DiffLineOp::Delete(line) => DiffLineOp::Delete(line),
            },
        ));
        rewritten.extend_from_slice(
            &ops[matched_insert_start..matched_insert_start + matched_attr_start],
        );
        rewritten.extend(
            ops[matched_insert_start + matched_attr_start..matched_insert_end]
                .iter()
                .copied()
                .map(|op| match op {
                    DiffLineOp::Insert(line) => DiffLineOp::Equal(line),
                    _ => op,
                }),
        );
        idx = matched_insert_end;
    }
    *ops = rewritten;
}

fn normalize_diff_declaration_alignment_in_place<'a>(ops: &mut Vec<DiffLineOp<'a>>) {
    for _ in 0..3 {
        normalize_diff_equal_attribute_suffix_before_inserted_declaration_in_place(ops);
        normalize_diff_inserted_declaration_with_shared_body_prefix_against_following_declaration_in_place(ops);
        normalize_diff_trailing_inserted_attributes_before_equal_declaration_in_place(ops);
        normalize_diff_insert_prefix_duplicate_to_following_equal_context_in_place(ops);
    }
    normalize_diff_equal_body_after_inserted_declaration_before_following_declaration_in_place(ops);
    normalize_diff_equal_attribute_suffix_before_inserted_declaration_in_place(ops);
}

fn normalize_diff_trailing_inserted_attributes_before_equal_declaration_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let equal_len = idx - equal_start;
        if equal_len == 0
            || !matches!(
                ops[equal_start],
                DiffLineOp::Equal(line) if is_direct_declaration_line_bytes(line)
            )
        {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            continue;
        }

        let trailing_attr_len =
            inserted_attribute_suffix_len(&ops[insert_start..insert_start + insert_len]);
        if trailing_attr_len == 0 {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            continue;
        }
        if insert_run_contains_direct_declaration_before_suffix(
            &ops[insert_start..insert_start + insert_len],
            trailing_attr_len,
        ) {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            continue;
        }

        rewritten
            .extend_from_slice(&ops[insert_start..insert_start + insert_len - trailing_attr_len]);
        rewritten.extend(
            ops[insert_start + insert_len - trailing_attr_len..insert_start + insert_len]
                .iter()
                .copied()
                .map(|op| match op {
                    DiffLineOp::Insert(line) => DiffLineOp::Equal(line),
                    _ => op,
                }),
        );
        rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
    }
    *ops = rewritten;
}

fn inserted_attribute_suffix_len(insert_run: &[DiffLineOp<'_>]) -> usize {
    let mut suffix_len = 0usize;
    for op in insert_run.iter().rev() {
        match *op {
            DiffLineOp::Insert(line) if is_attribute_line_bytes(line) => suffix_len += 1,
            _ => break,
        }
    }
    suffix_len
}

fn insert_run_contains_direct_declaration_before_suffix(
    insert_run: &[DiffLineOp<'_>],
    suffix_len: usize,
) -> bool {
    let body_len = insert_run.len().saturating_sub(suffix_len);
    insert_run[..body_len].iter().any(|op| match *op {
        DiffLineOp::Insert(line) => is_direct_declaration_line_bytes(line),
        _ => false,
    })
}

fn insert_run_prefix_contains_attribute_with_internal_declaration(
    insert_run: &[DiffLineOp<'_>],
    prefix_len: usize,
) -> bool {
    prefix_len > 0
        && matches!(
            insert_run.first().copied(),
            Some(DiffLineOp::Insert(line)) if is_attribute_line_bytes(line)
        )
        && insert_run_contains_direct_declaration(&insert_run[prefix_len..])
}

fn first_deleted_direct_declaration_line<'a>(delete_run: &[DiffLineOp<'a>]) -> Option<&'a [u8]> {
    delete_run.iter().find_map(|op| match *op {
        DiffLineOp::Delete(line) if is_direct_declaration_line_bytes(line) => Some(line),
        _ => None,
    })
}

fn insert_run_starts_with_direct_declaration(insert_run: &[DiffLineOp<'_>]) -> bool {
    matches!(
        insert_run.first().copied(),
        Some(DiffLineOp::Insert(line)) if is_direct_declaration_line_bytes(line)
    )
}

fn find_later_insert_run_with_matching_attribute_and_decl<'a>(
    ops: &[DiffLineOp<'a>],
    start: usize,
    attr_prefix: &[DiffLineOp<'_>],
    deleted_decl_line: &[u8],
) -> Option<(usize, usize, usize)> {
    let mut idx = start;
    while idx < ops.len() {
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        if insert_start == idx {
            break;
        }
        let Some(old_decl_offset) = inserted_decl_offset_with_matching_attribute_prefix(
            &ops[insert_start..idx],
            attr_prefix,
            deleted_decl_line,
        ) else {
            continue;
        };
        let attr_start = old_decl_offset - attr_prefix.len();
        return Some((insert_start, idx, attr_start));
    }
    None
}

fn inserted_decl_offset_with_matching_attribute_prefix(
    insert_run: &[DiffLineOp<'_>],
    attr_prefix: &[DiffLineOp<'_>],
    deleted_decl_line: &[u8],
) -> Option<usize> {
    insert_run
        .iter()
        .enumerate()
        .find_map(|(idx, op)| match *op {
            DiffLineOp::Insert(line) if line == deleted_decl_line => {
                let attr_start = idx.checked_sub(attr_prefix.len())?;
                attribute_insert_prefix_matches_equal_suffix(
                    &insert_run[attr_start..idx],
                    attr_prefix,
                )
                .then_some(idx)
            }
            _ => None,
        })
}

fn attribute_insert_prefix_matches_equal_suffix(
    insert_prefix: &[DiffLineOp<'_>],
    equal_suffix: &[DiffLineOp<'_>],
) -> bool {
    insert_prefix
        .iter()
        .zip(equal_suffix)
        .all(|(inserted, equal)| match (*inserted, *equal) {
            (DiffLineOp::Insert(inserted_line), DiffLineOp::Equal(equal_line)) => {
                inserted_line == equal_line
            }
            _ => false,
        })
}

fn normalize_diff_insert_prefix_duplicate_to_following_equal_context_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let equal_len = idx - equal_start;
        if equal_len == 0 {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            continue;
        }

        if insert_run_starts_with_optional_blank_then_attribute(
            &ops[insert_start..insert_start + insert_len],
        ) && insert_run_contains_direct_declaration(
            &ops[insert_start..insert_start + insert_len],
        ) {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            continue;
        }

        let common_prefix_len = insert_equal_context_tail_prefix_len(
            &ops[insert_start..insert_start + insert_len],
            &ops[equal_start..equal_start + equal_len],
        );
        if common_prefix_len > 0
            && common_prefix_len < insert_len
            && !insert_run_prefix_contains_attribute_with_internal_declaration(
                &ops[insert_start..insert_start + insert_len],
                common_prefix_len,
            )
            && insert_remainder_starts_with_declaration_or_attribute(
                &ops[insert_start + common_prefix_len..insert_start + insert_len],
            )
        {
            rewritten.extend(
                ops[insert_start..insert_start + common_prefix_len]
                    .iter()
                    .copied()
                    .map(|op| match op {
                        DiffLineOp::Insert(line) => DiffLineOp::Equal(line),
                        _ => op,
                    }),
            );
            rewritten.extend_from_slice(
                &ops[insert_start + common_prefix_len..insert_start + insert_len],
            );
            rewritten
                .extend_from_slice(&ops[equal_start + common_prefix_len..equal_start + equal_len]);
            continue;
        }

        rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
        rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
    }
    *ops = rewritten;
}

fn insert_equal_context_tail_prefix_len(
    insert_run: &[DiffLineOp<'_>],
    equal_run: &[DiffLineOp<'_>],
) -> usize {
    let mut matched_len = 0usize;
    let mut saw_function_epilogue = false;
    for (inserted, equal) in insert_run.iter().zip(equal_run) {
        let (DiffLineOp::Insert(inserted_line), DiffLineOp::Equal(equal_line)) =
            (*inserted, *equal)
        else {
            break;
        };
        if inserted_line != equal_line {
            break;
        }
        if matched_len == 0 && is_insert_block_boundary_line_bytes(inserted_line) {
            break;
        }
        if saw_function_epilogue && is_insert_block_boundary_line_bytes(inserted_line) {
            break;
        }
        matched_len += 1;
        if is_function_epilogue_line(inserted_line) {
            saw_function_epilogue = true;
        }
    }
    saw_function_epilogue.then_some(matched_len).unwrap_or(0)
}

fn normalize_diff_equal_body_after_inserted_declaration_before_following_declaration_in_place<
    'a,
>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut idx = 0usize;
    while idx < ops.len() {
        while idx < ops.len() && !matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        if idx >= ops.len() {
            break;
        }
        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_start > 0 && matches!(ops[insert_start - 1], DiffLineOp::Delete(_)) {
            continue;
        }
        if (!insert_run_starts_with_attribute(&ops[insert_start..insert_start + insert_len])
            && !insert_run_starts_with_direct_declaration(
                &ops[insert_start..insert_start + insert_len],
            ))
            || !insert_run_contains_direct_declaration(
                &ops[insert_start..insert_start + insert_len],
            )
        {
            continue;
        }

        let right_equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let right_equal_len = idx - right_equal_start;
        if right_equal_len == 0 {
            continue;
        }
        if matches!(
            ops[right_equal_start],
            DiffLineOp::Equal(line) if is_insert_block_boundary_line_bytes(line)
        ) {
            continue;
        }

        let Some(next_decl_offset) = equal_run_prefix_len_before_following_declaration(
            &ops[right_equal_start..right_equal_start + right_equal_len],
        ) else {
            continue;
        };
        for op in &mut ops[right_equal_start..right_equal_start + next_decl_offset] {
            if let DiffLineOp::Equal(line) = *op {
                *op = DiffLineOp::Insert(line);
            }
        }
    }
}

fn equal_run_prefix_len_before_following_declaration(
    equal_run: &[DiffLineOp<'_>],
) -> Option<usize> {
    let mut saw_function_epilogue = false;
    for (idx, op) in equal_run.iter().enumerate() {
        let DiffLineOp::Equal(line) = *op else {
            return None;
        };
        if idx > 0 && is_insert_block_boundary_line_bytes(line) && saw_function_epilogue {
            return Some(idx);
        }
        if is_function_epilogue_line(line) {
            saw_function_epilogue = true;
        }
    }
    None
}

fn insert_run_starts_with_attribute(insert_run: &[DiffLineOp<'_>]) -> bool {
    matches!(
        insert_run.first().copied(),
        Some(DiffLineOp::Insert(line)) if is_attribute_line_bytes(line)
    )
}

fn insert_run_starts_with_optional_blank_then_attribute(insert_run: &[DiffLineOp<'_>]) -> bool {
    let first_non_blank = insert_run.iter().find(|op| match **op {
        DiffLineOp::Insert(line) => !is_blank_diff_line(line),
        _ => true,
    });
    matches!(
        first_non_blank.copied(),
        Some(DiffLineOp::Insert(line)) if is_attribute_line_bytes(line)
    )
}

pub(crate) fn normalize_diff_insert_prefix_structural_duplicate_to_following_equal_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let equal_len = idx - equal_start;
        if equal_len == 0 {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            continue;
        }

        if insert_run_starts_with_optional_blank_then_attribute(
            &ops[insert_start..insert_start + insert_len],
        ) && insert_run_contains_direct_declaration(
            &ops[insert_start..insert_start + insert_len],
        ) {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
            continue;
        }

        let prefix_len = common_structural_insert_equal_prefix_len(
            &ops[insert_start..insert_start + insert_len],
            &ops[equal_start..equal_start + equal_len],
        );
        if prefix_len > 0
            && prefix_len < insert_len
            && prefix_len < equal_len
            && !insert_run_prefix_contains_attribute_with_internal_declaration(
                &ops[insert_start..insert_start + insert_len],
                prefix_len,
            )
            && insert_and_equal_eventually_diverge_after_prefix(
                &ops[insert_start..insert_start + insert_len],
                &ops[equal_start..equal_start + equal_len],
                prefix_len,
            )
        {
            rewritten.extend(
                ops[insert_start..insert_start + prefix_len]
                    .iter()
                    .copied()
                    .map(|op| match op {
                        DiffLineOp::Insert(line) => DiffLineOp::Equal(line),
                        _ => op,
                    }),
            );
            rewritten.extend_from_slice(&ops[insert_start + prefix_len..insert_start + insert_len]);
            rewritten.extend_from_slice(&ops[insert_start..insert_start + prefix_len]);
            rewritten.extend_from_slice(&ops[equal_start + prefix_len..equal_start + equal_len]);
        } else {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            rewritten.extend_from_slice(&ops[equal_start..equal_start + equal_len]);
        }
    }
    *ops = rewritten;
}

pub(crate) fn normalize_diff_inserted_epilogue_prefix_rebind_from_following_equal_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut idx = 0usize;
    while idx < ops.len() {
        while idx < ops.len() && !matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        if idx >= ops.len() {
            break;
        }

        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            continue;
        }

        let equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let equal_len = idx - equal_start;
        if equal_len == 0 {
            continue;
        }

        let insert_run = &ops[insert_start..insert_start + insert_len];
        let equal_run = &ops[equal_start..equal_start + equal_len];
        if !insert_run_contains_direct_declaration(insert_run)
            || !equal_run
                .iter()
                .all(|op| matches!(*op, DiffLineOp::Equal(line) if is_function_epilogue_line(line)))
        {
            continue;
        }

        let mut matched_len = 0usize;
        while matched_len < insert_run.len() && matched_len < equal_run.len() {
            let (DiffLineOp::Insert(inserted_line), DiffLineOp::Equal(equal_line)) =
                (insert_run[matched_len], equal_run[matched_len])
            else {
                break;
            };
            if inserted_line != equal_line || !is_function_epilogue_line(inserted_line) {
                break;
            }
            matched_len += 1;
        }
        if matched_len == 0 {
            continue;
        }

        for op in &mut ops[insert_start..insert_start + matched_len] {
            if let DiffLineOp::Insert(line) = *op {
                *op = DiffLineOp::Equal(line);
            }
        }
        for op in &mut ops[equal_start..equal_start + matched_len] {
            if let DiffLineOp::Equal(line) = *op {
                *op = DiffLineOp::Insert(line);
            }
        }
    }
}

pub(crate) fn normalize_diff_epilogue_ok_declaration_sandwich_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let left_equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let left_equal_len = idx - left_equal_start;
        if left_equal_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let ok_insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let ok_insert_len = idx - ok_insert_start;
        if ok_insert_len == 0 {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            continue;
        }

        let right_equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let right_equal_len = idx - right_equal_start;
        if right_equal_len == 0 {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            rewritten.extend_from_slice(&ops[ok_insert_start..ok_insert_start + ok_insert_len]);
            continue;
        }

        let decl_insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let decl_insert_len = idx - decl_insert_start;
        if decl_insert_len == 0 {
            rewritten.extend_from_slice(&ops[left_equal_start..left_equal_start + left_equal_len]);
            rewritten.extend_from_slice(&ops[ok_insert_start..ok_insert_start + ok_insert_len]);
            rewritten
                .extend_from_slice(&ops[right_equal_start..right_equal_start + right_equal_len]);
            continue;
        }

        let left = &ops[left_equal_start..left_equal_start + left_equal_len];
        let ok_insert = &ops[ok_insert_start..ok_insert_start + ok_insert_len];
        let right = &ops[right_equal_start..right_equal_start + right_equal_len];
        let decl_insert = &ops[decl_insert_start..decl_insert_start + decl_insert_len];
        if left
            .iter()
            .all(|op| matches!(*op, DiffLineOp::Equal(line) if is_function_epilogue_line(line)))
            && ok_insert
                .iter()
                .all(|op| matches!(*op, DiffLineOp::Insert(line) if is_ok_rebind_line_bytes(line)))
            && right
                .iter()
                .all(|op| matches!(*op, DiffLineOp::Equal(line) if is_function_epilogue_line(line)))
            && decl_insert.first().is_some_and(
                |op| matches!(*op, DiffLineOp::Insert(line) if is_rebindable_structural_line_bytes(line)),
            )
        {
            rewritten.extend(left.iter().copied().map(|op| match op {
                DiffLineOp::Equal(line) => DiffLineOp::Insert(line),
                _ => op,
            }));
            rewritten.extend_from_slice(ok_insert);
            rewritten.extend(right.iter().copied().map(|op| match op {
                DiffLineOp::Equal(line) => DiffLineOp::Insert(line),
                _ => op,
            }));
            rewritten.extend_from_slice(decl_insert);
            continue;
        }

        rewritten.extend_from_slice(left);
        rewritten.extend_from_slice(ok_insert);
        rewritten.extend_from_slice(right);
        rewritten.extend_from_slice(decl_insert);
    }
    *ops = rewritten;
}

pub(crate) fn normalize_diff_inserted_structural_suffix_before_declaration_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let equal_len = idx - equal_start;
        if equal_len == 0 {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            continue;
        }

        let insert_run = &ops[insert_start..insert_start + insert_len];
        let equal_run = &ops[equal_start..equal_start + equal_len];
        let structural_suffix_len = insert_run
            .iter()
            .rev()
            .take_while(
                |op| matches!(**op, DiffLineOp::Insert(line) if is_function_epilogue_line(line)),
            )
            .count();
        if structural_suffix_len > 0
            && structural_suffix_len < insert_len
            && insert_run_contains_direct_declaration(insert_run)
            && !insert_run_starts_with_attribute(insert_run)
            && equal_run.first().is_some_and(
                |op| matches!(*op, DiffLineOp::Equal(line) if is_direct_declaration_line_bytes(line)),
            )
        {
            rewritten.extend_from_slice(
                &ops[insert_start..insert_start + insert_len - structural_suffix_len],
            );
            rewritten.extend(
                ops[insert_start + insert_len - structural_suffix_len..insert_start + insert_len]
                    .iter()
                    .copied()
                    .map(|op| match op {
                        DiffLineOp::Insert(line) => DiffLineOp::Equal(line),
                        _ => op,
                    }),
            );
            rewritten.extend_from_slice(equal_run);
            continue;
        }

        rewritten.extend_from_slice(insert_run);
        rewritten.extend_from_slice(equal_run);
    }
    *ops = rewritten;
}

pub(crate) fn normalize_diff_equal_epilogue_before_inserted_declaration_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let equal_len = idx - equal_start;
        if equal_len == 0 {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            continue;
        }

        let equal_run = &ops[equal_start..equal_start + equal_len];
        let epilogue_prefix_len = equal_run
            .iter()
            .take_while(
                |op| matches!(**op, DiffLineOp::Equal(line) if is_function_epilogue_line(line)),
            )
            .count();
        let declaration_follows_in_insert_run = if epilogue_prefix_len == equal_len {
            idx < ops.len()
                && matches!(
                    ops[idx],
                    DiffLineOp::Insert(line) if is_direct_declaration_line_bytes(line)
                )
        } else {
            false
        };

        if epilogue_prefix_len > 0
            && insert_run_contains_direct_declaration(&ops[insert_start..insert_start + insert_len])
            && declaration_follows_in_insert_run
        {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            rewritten.extend(
                equal_run[..epilogue_prefix_len]
                    .iter()
                    .copied()
                    .map(|op| match op {
                        DiffLineOp::Equal(line) => DiffLineOp::Insert(line),
                        _ => op,
                    }),
            );
            rewritten.extend_from_slice(&equal_run[epilogue_prefix_len..]);
            continue;
        }

        rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
        rewritten.extend_from_slice(equal_run);
    }
    *ops = rewritten;
}

pub(crate) fn normalize_diff_equal_epilogue_before_related_equal_declaration_in_place<'a>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut rewritten = Vec::with_capacity(ops.len());
    let mut idx = 0usize;
    while idx < ops.len() {
        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        if insert_len == 0 {
            rewritten.push(ops[idx]);
            idx += 1;
            continue;
        }

        let equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let equal_len = idx - equal_start;
        if equal_len == 0 {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            continue;
        }

        let insert_decl =
            first_inserted_declaration_name(&ops[insert_start..insert_start + insert_len]);
        let equal_run = &ops[equal_start..equal_start + equal_len];
        let epilogue_prefix_len = equal_run
            .iter()
            .take_while(
                |op| matches!(**op, DiffLineOp::Equal(line) if is_function_epilogue_line(line)),
            )
            .count();
        let next_equal_decl = if epilogue_prefix_len < equal_len {
            match equal_run[epilogue_prefix_len] {
                DiffLineOp::Equal(line) => function_name_from_declaration_line(line),
                _ => None,
            }
        } else {
            None
        };

        if epilogue_prefix_len > 0
            && insert_decl.is_some_and(|name| declaration_name_segment_count(name) <= 3)
            && insert_decl
                .zip(next_equal_decl)
                .is_some_and(|(left, right)| declaration_names_share_module_prefix(left, right))
        {
            rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
            rewritten.extend(
                equal_run[..epilogue_prefix_len]
                    .iter()
                    .copied()
                    .map(|op| match op {
                        DiffLineOp::Equal(line) => DiffLineOp::Insert(line),
                        _ => op,
                    }),
            );
            rewritten.extend_from_slice(&equal_run[epilogue_prefix_len..]);
            continue;
        }

        rewritten.extend_from_slice(&ops[insert_start..insert_start + insert_len]);
        rewritten.extend_from_slice(equal_run);
    }
    *ops = rewritten;
}

pub(crate) fn normalize_diff_equal_epilogue_after_inserted_declaration_with_equal_context_in_place<
    'a,
>(
    ops: &mut Vec<DiffLineOp<'a>>,
) {
    let mut idx = 0usize;
    while idx < ops.len() {
        while idx < ops.len() && !matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        if idx >= ops.len() {
            break;
        }
        let insert_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Insert(_)) {
            idx += 1;
        }
        let insert_len = idx - insert_start;
        let right_equal_start = idx;
        while idx < ops.len() && matches!(ops[idx], DiffLineOp::Equal(_)) {
            idx += 1;
        }
        let right_equal_len = idx - right_equal_start;
        if right_equal_len == 0 {
            continue;
        }

        let mut left_equal_start = insert_start;
        while left_equal_start > 0 && matches!(ops[left_equal_start - 1], DiffLineOp::Equal(_)) {
            left_equal_start -= 1;
        }
        let left_equal_run = &ops[left_equal_start..insert_start];
        if left_equal_run.is_empty() {
            continue;
        }
        let right_equal_run = &ops[right_equal_start..right_equal_start + right_equal_len];
        let left_epilogue_suffix_len = left_equal_run
            .iter()
            .rev()
            .take_while(
                |op| matches!(**op, DiffLineOp::Equal(line) if is_function_epilogue_line(line)),
            )
            .count();
        let right_epilogue_prefix_len = right_equal_run
            .iter()
            .take_while(
                |op| matches!(**op, DiffLineOp::Equal(line) if is_function_epilogue_line(line)),
            )
            .count();
        let right_equal_decl_follows = right_epilogue_prefix_len < right_equal_len
            && matches!(
                right_equal_run[right_epilogue_prefix_len],
                DiffLineOp::Equal(line) if is_direct_declaration_line_bytes(line)
            );

        if left_epilogue_suffix_len > 0
            && right_epilogue_prefix_len > 0
            && insert_run_contains_direct_declaration(&ops[insert_start..insert_start + insert_len])
            && right_equal_decl_follows
        {
            for op in &mut ops[right_equal_start..right_equal_start + right_epilogue_prefix_len] {
                if let DiffLineOp::Equal(line) = *op {
                    *op = DiffLineOp::Insert(line);
                }
            }
        }
    }
}

fn equal_run_suffix_is_not_only_function_epilogue(equal_suffix: &[DiffLineOp<'_>]) -> bool {
    equal_suffix.iter().any(|op| match *op {
        DiffLineOp::Equal(line) => !is_function_epilogue_line(line),
        _ => true,
    })
}

fn equal_run_prefix_starts_with_opener_line(equal_prefix: &[DiffLineOp<'_>]) -> bool {
    match equal_prefix.first().copied() {
        Some(DiffLineOp::Equal(line)) => is_rebindable_structural_line_bytes(line),
        _ => false,
    }
}

fn equal_run_starts_with_opener_line(equal_run: &[DiffLineOp<'_>]) -> bool {
    equal_run_prefix_starts_with_opener_line(equal_run)
}

fn equal_run_starts_with_insert_block_boundary(equal_run: &[DiffLineOp<'_>]) -> bool {
    match equal_run.first().copied() {
        Some(DiffLineOp::Equal(line)) => is_insert_block_boundary_line_bytes(line),
        _ => false,
    }
}

fn equal_run_suffix_is_preceded_by_epilogue_line(
    equal_run: &[DiffLineOp<'_>],
    suffix_len: usize,
) -> bool {
    if suffix_len >= equal_run.len() {
        return false;
    }
    match equal_run.get(equal_run.len() - suffix_len - 1).copied() {
        Some(DiffLineOp::Equal(line)) => is_function_epilogue_line(line),
        _ => false,
    }
}

fn equal_run_suffix_starts_with_rebindable_prefix_line(equal_suffix: &[DiffLineOp<'_>]) -> bool {
    match equal_suffix.first().copied() {
        Some(DiffLineOp::Equal(line)) => {
            is_rebindable_structural_line_bytes(line)
                || is_ok_rebind_line_bytes(line)
                || (is_function_epilogue_line(line)
                    && equal_suffix.iter().skip(1).any(|op| match *op {
                        DiffLineOp::Equal(next_line) => is_ok_rebind_line_bytes(next_line),
                        _ => false,
                    }))
        }
        _ => false,
    }
}

fn equal_run_suffix_starts_with_ok_line(equal_suffix: &[DiffLineOp<'_>]) -> bool {
    match equal_suffix.first().copied() {
        Some(DiffLineOp::Equal(line)) => is_ok_rebind_line_bytes(line),
        _ => false,
    }
}

fn is_rebindable_structural_line_bytes(line: &[u8]) -> bool {
    let trimmed = strip_trailing_lf(line)
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .collect::<Vec<_>>();
    (trimmed.starts_with(b"#[") && !trimmed.starts_with(b"#[test"))
        || is_direct_declaration_trimmed(&trimmed)
}

fn is_insert_block_boundary_line_bytes(line: &[u8]) -> bool {
    let trimmed = strip_trailing_lf(line)
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .collect::<Vec<_>>();
    trimmed.starts_with(b"#[") || is_direct_declaration_trimmed(&trimmed)
}

fn is_attribute_line_bytes(line: &[u8]) -> bool {
    strip_trailing_lf(line)
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .collect::<Vec<_>>()
        .starts_with(b"#[")
}

fn is_direct_declaration_line_bytes(line: &[u8]) -> bool {
    let trimmed = strip_trailing_lf(line)
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .collect::<Vec<_>>();
    is_direct_declaration_trimmed(&trimmed)
}

fn is_direct_declaration_trimmed(trimmed: &[u8]) -> bool {
    trimmed.ends_with(b"(")
        || trimmed.starts_with(b"fn ")
        || trimmed.starts_with(b"pub fn ")
        || trimmed.starts_with(b"pub(crate) fn ")
        || trimmed.starts_with(b"pub(super) fn ")
        || trimmed.starts_with(b"struct ")
        || trimmed.starts_with(b"enum ")
        || trimmed.starts_with(b"trait ")
        || trimmed.starts_with(b"impl ")
}

fn is_ok_rebind_line_bytes(line: &[u8]) -> bool {
    let trimmed = strip_trailing_lf(line)
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .collect::<Vec<_>>();
    trimmed.starts_with(b"Ok(") || trimmed.starts_with(b"Ok::<")
}

enum EitherSuffixLens {
    Ascending(std::ops::RangeInclusive<usize>),
    Descending(std::ops::RangeInclusive<usize>),
}

impl Iterator for EitherSuffixLens {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            EitherSuffixLens::Ascending(range) => range.next(),
            EitherSuffixLens::Descending(range) => range.next_back(),
        }
    }
}

fn is_function_epilogue_line(line: &[u8]) -> bool {
    if is_blank_diff_line(line) {
        return true;
    }
    let trimmed = strip_trailing_lf(line)
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .collect::<Vec<_>>();
    trimmed == b"}"
        || trimmed == b"},"
        || trimmed == b"};"
        || trimmed.starts_with(b"Ok(")
        || trimmed.starts_with(b"Ok::<")
}

fn insert_run_contains_direct_declaration(insert_run: &[DiffLineOp<'_>]) -> bool {
    insert_run.iter().any(|op| match *op {
        DiffLineOp::Insert(line) => is_direct_declaration_line_bytes(line),
        _ => false,
    })
}

fn first_inserted_declaration_name<'a>(insert_run: &[DiffLineOp<'a>]) -> Option<&'a str> {
    insert_run.iter().find_map(|op| match *op {
        DiffLineOp::Insert(line) => function_name_from_declaration_line(line),
        _ => None,
    })
}

fn function_name_from_declaration_line(line: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(strip_trailing_lf(line))
        .ok()?
        .trim_start();
    let after_fn = text
        .strip_prefix("fn ")
        .or_else(|| text.strip_prefix("pub fn "))
        .or_else(|| text.strip_prefix("pub(crate) fn "))
        .or_else(|| text.strip_prefix("pub(super) fn "))?;
    let end = after_fn.find('(')?;
    Some(after_fn[..end].trim_end())
}

fn declaration_name_segment_count(name: &str) -> usize {
    name.split('_')
        .filter(|segment| !segment.is_empty())
        .count()
}

fn declaration_names_share_module_prefix(left: &str, right: &str) -> bool {
    let left_segments = left
        .split('_')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let right_segments = right
        .split('_')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    left_segments.len() >= 2
        && right_segments.len() >= 2
        && left_segments[0] == right_segments[0]
        && left_segments[1] == right_segments[1]
}

fn insert_run_suffix_matches_equal_run(
    equal_run: &[DiffLineOp<'_>],
    insert_run: &[DiffLineOp<'_>],
) -> bool {
    equal_run
        .iter()
        .zip(&insert_run[insert_run.len() - equal_run.len()..])
        .all(|(equal, inserted)| match (*equal, *inserted) {
            (DiffLineOp::Equal(equal_line), DiffLineOp::Insert(inserted_line)) => {
                equal_line == inserted_line
            }
            _ => false,
        })
}

fn common_structural_insert_equal_prefix_len(
    insert_run: &[DiffLineOp<'_>],
    equal_run: &[DiffLineOp<'_>],
) -> usize {
    insert_run
        .iter()
        .zip(equal_run)
        .take_while(|(inserted, equal)| match (**inserted, **equal) {
            (DiffLineOp::Insert(inserted_line), DiffLineOp::Equal(equal_line)) => {
                inserted_line == equal_line && is_structural_prefix_line(inserted_line)
            }
            _ => false,
        })
        .count()
}

fn insert_and_equal_eventually_diverge_after_prefix(
    insert_run: &[DiffLineOp<'_>],
    equal_run: &[DiffLineOp<'_>],
    prefix_len: usize,
) -> bool {
    for (inserted, equal) in insert_run[prefix_len..]
        .iter()
        .zip(&equal_run[prefix_len..])
    {
        match (*inserted, *equal) {
            (DiffLineOp::Insert(inserted_line), DiffLineOp::Equal(equal_line)) => {
                if inserted_line != equal_line {
                    return true;
                }
            }
            _ => return true,
        }
    }
    insert_run.len() != equal_run.len()
}

fn is_structural_prefix_line(line: &[u8]) -> bool {
    let trimmed = strip_trailing_lf(line)
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    trimmed.is_empty()
        || trimmed
            .iter()
            .all(|byte| matches!(byte, b'}' | b')' | b']' | b',' | b';'))
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
    use std::fs;

    use tempfile::TempDir;
    use zmin_cli_runtime::GitRepo;
    use zmin_git_core::{
        GitHashAlgorithm, GitIndex, IndexDiffEntry, IndexDiffStatus, IndexEntry, IndexMode,
        LooseObjectStore, ObjectId,
    };

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

    fn test_repo(root: &std::path::Path) -> GitRepo {
        let git_dir = root.join(".git");
        let objects_dir = git_dir.join("objects");
        std::fs::create_dir_all(&objects_dir).expect("test repo objects");
        GitRepo {
            root: root.to_path_buf(),
            git_dir,
            objects_dir,
            index_path: root.join(".git").join("index"),
        }
    }

    fn debug_line_tags(old: &[u8], new: &[u8]) -> Vec<(u8, String)> {
        let old_lines = split_diff_lines(old);
        let new_lines = split_diff_lines(new);
        diff_line_ops_with_whitespace(&old_lines, &new_lines, DiffWhitespaceMode::None)
            .into_iter()
            .map(|op| match op {
                DiffLineOp::Equal(line) => (b' ', String::from_utf8_lossy(line).into_owned()),
                DiffLineOp::Delete(line) => (b'-', String::from_utf8_lossy(line).into_owned()),
                DiffLineOp::Insert(line) => (b'+', String::from_utf8_lossy(line).into_owned()),
            })
            .collect()
    }

    fn rendered_hunk_op_tags(old: &[u8], new: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let old_lines = split_diff_lines(old);
        let new_lines = split_diff_lines(new);
        let ops = diff_line_ops_with_whitespace(&old_lines, &new_lines, DiffWhitespaceMode::None);
        let ranges = unified_hunk_ranges(&ops, 3, 0);
        let (start, end) = ranges[0];
        let mut render_ops = ops[start..end].to_vec();
        normalize_diff_declaration_alignment_in_place(&mut render_ops);
        normalize_diff_equal_run_suffix_to_insert_prefix_duplicate_in_place(&mut render_ops);
        diff_line_op_tags(&render_ops)
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
    fn non_rust_path_diff_matches_stock_shape_for_format_patch_ignore_if_series_tail() {
        let old_lines = split_diff_lines(b"1\n2\n5\n6\nA\nB\nC\n7\n8\n9\n10\nD\nE\nF\n");
        let new_lines = split_diff_lines(b"5\n6\n1\n2\n3\nA\n4\nB\nC\n7\n8\n9\n10\nD\nE\nF\n");
        let ops = diff_line_ops_for_path_with_whitespace(
            &old_lines,
            &new_lines,
            DiffWhitespaceMode::None,
            "file",
        );

        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (b'-', b"1\n".to_vec()),
                (b'-', b"2\n".to_vec()),
                (b' ', b"5\n".to_vec()),
                (b' ', b"6\n".to_vec()),
                (b'+', b"1\n".to_vec()),
                (b'+', b"2\n".to_vec()),
                (b'+', b"3\n".to_vec()),
                (b' ', b"A\n".to_vec()),
                (b'+', b"4\n".to_vec()),
                (b' ', b"B\n".to_vec()),
                (b' ', b"C\n".to_vec()),
                (b' ', b"7\n".to_vec()),
                (b' ', b"8\n".to_vec()),
                (b' ', b"9\n".to_vec()),
                (b' ', b"10\n".to_vec()),
                (b' ', b"D\n".to_vec()),
                (b' ', b"E\n".to_vec()),
                (b' ', b"F\n".to_vec()),
            ]
        );
    }

    #[test]
    fn diff_word_ops_keeps_current_word_alignment_shape() {
        let old = split_word_diff_tokens(b"alpha beta gamma", None).expect("old tokens");
        let new = split_word_diff_tokens(b"beta delta gamma", None).expect("new tokens");
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
    fn diff_line_ops_keeps_inserted_test_attribute_with_inserted_test_body() {
        let old_lines = split_diff_lines(
            br#"    assert!(ready);
}

#[test]
fn next_test() {
    assert!(next);
}
"#,
        );
        let new_lines = split_diff_lines(
            br#"    assert!(ready);
}

#[test]
fn inserted_test() {
    assert!(inserted);
}

#[test]
fn next_test() {
    assert!(next);
}
"#,
        );

        let ops = diff_line_ops(&old_lines, &new_lines);
        let first_insert = ops
            .iter()
            .find_map(|op| match *op {
                DiffLineOp::Insert(line) => Some(line),
                _ => None,
            })
            .expect("inserted test attribute");

        assert_eq!(first_insert, b"#[test]\n");
    }

    #[test]
    fn diff_line_ops_keeps_previous_function_tail_as_equal_context_before_inserted_test() {
        let old_lines = split_diff_lines(
            br#"            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file {}\n",
                credentials.display()
            ),
        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with credential helper");

        assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
    }

    #[test]
    fn parsed_http_url_keeps_url_userinfo_before_credential_store() {
        assert!(next);
    }
"#,
        );
        let new_lines = split_diff_lines(
            br#"            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file {}\n",
                credentials.display()
            ),
        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with credential helper");

        assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
    }

    #[test]
    fn parsed_http_url_reads_credential_store_helper_basic_auth_with_quoted_file_path() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo(dir.path());
        let credentials_dir = dir.path().join("folder with spaces");
        std::fs::create_dir_all(&credentials_dir).expect("credentials dir");
        let credentials = credentials_dir.join("quoted credentials");
        std::fs::write(&credentials, "https://user:p%40ss@example.test\n").expect("credentials");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file '{}'\n",
                credentials.display()
            ),
        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with credential helper");

        assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
    }

    #[test]
    fn parsed_http_url_keeps_url_userinfo_before_credential_store() {
        assert!(next);
    }
"#,
        );

        let ops = diff_line_ops(&old_lines, &new_lines);
        let first_insert = ops
            .iter()
            .find_map(|op| match *op {
                DiffLineOp::Insert(line) => Some(line),
                _ => None,
            })
            .expect("inserted test header");

        assert_eq!(first_insert, b"    #[test]\n");
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

    #[test]
    fn rust_hunk_header_prefers_signature_lines_over_nearby_control_flow() {
        let old_lines = split_diff_lines(
            br#"pub(crate) fn managed_hooks(command: ManagedHooksCommand) -> Result<()> {
    match command {
        ManagedHooksCommand::Init => managed_hooks_init(),
    }
}
"#,
        );

        assert_eq!(
            rust_hunk_header(&old_lines, 3),
            Some(
                "pub(crate) fn managed_hooks(command: ManagedHooksCommand) -> Result<()> {".into()
            )
        );
    }

    #[test]
    fn markdown_hunk_header_matches_stock_style_context_pick() {
        let old_lines = split_diff_lines(
            br#"Implemented extensions include `zmin clone --instant`, managed `zmin hooks`
commands, `zmin repo` metadata summaries and CMS-style porcelain such as
`zmin save`, `zmin changes`, `zmin publish` and `zmin update`. Transport
tuning through `ZMIN_GIT_HTTP_VERSION` is tracked as a Zmin-only environment
control.
"#,
        );

        assert_eq!(
            markdown_hunk_header(&old_lines, 4),
            Some("commands, `zmin repo` metadata summaries and CMS-style porcelain such as".into())
        );
    }

    #[test]
    fn default_hunk_header_matches_stock_plain_text_rule() {
        let old_lines = split_diff_lines(
            br#"1
2
C
D
E
"#,
        );

        assert_eq!(default_hunk_header(&old_lines, 4), Some("C".into()));
        assert_eq!(default_hunk_header(&old_lines, 3), None);
    }

    #[test]
    fn default_hunk_header_skips_non_candidate_plain_text_lines() {
        let old_lines = split_diff_lines(
            br#"1
2
C
4
5
6
7
8
9
10
11
"#,
        );

        assert_eq!(default_hunk_header(&old_lines, 9), Some("C".into()));
    }

    #[test]
    fn rust_hunk_header_matches_stock_style_context_pick() {
        let old_lines = split_diff_lines(
            br##"const MANAGED_HOOK_NAMES: &[&str] = &[
    "pre-commit",
    "commit-msg",
    "pre-push",
    "post-checkout",
    "post-merge",
];
const MANAGED_HOOK_MARKER: &str = "# zmin-managed-hook";
"##,
        );

        assert_eq!(
            rust_hunk_header(&old_lines, 6),
            Some("const MANAGED_HOOK_NAMES: &[&str] = &[".into())
        );
    }

    #[test]
    fn unified_hunk_matches_stock_shape_for_readme_replace_block() {
        let old = br#"tuning through `ZMIN_GIT_HTTP_VERSION` is tracked as a Zmin-only environment
control.

The staged-file hook runner is planned as a Zmin-only extension, with an API
shape like `zmin hooks run pre-commit --staged -- command ...`. It will be
tracked below the extension inventory, not in the Git compatibility matrix.

## Preview Limits

Zmin works with regular Git repositories and existing Git remotes. This preview
does not include Git LFS, reftable repositories or official package-manager
installs.

Some edge-case options and environments still need more coverage. Keep a
current backup before using preview builds on important repositories.

## Speed Snapshot
"#;
        let new = br#"tuning through `ZMIN_GIT_HTTP_VERSION` is tracked as a Zmin-only environment
control.

The staged-file hook runner is available as a Zmin-only extension, including
preview, extension-filtered execution, and dry-run command surfaces. It is
tracked below the extension inventory, not in the Git compatibility matrix.

## Preview Limits

Zmin works with regular Git repositories and existing Git remotes. This preview
does not claim full Git LFS product parity, reftable repositories, or official
package-manager installs. Basic Git-LFS-style pointer workflows through
configured `filter.lfs.process` are verified for `add`, `checkout`, and
`cat-file --filters`. Built-in local `git lfs` foundation coverage is also
verified for `version`, `env`, `install --local --skip-repo`,
`install --local --skip-smudge`, `track`, `untrack`, `ls-files`, and local
`pre-push` stdin-shape validation. Network LFS transfer flows, batch API/auth,
and broader `git lfs ...` product parity remain out of scope. Repo-configured
`credential.helper=store` and
`credential.helper=cache` flows are verified for `git credential
fill|approve|reject`, but broader authenticated enterprise transport scenarios
still need dedicated gates.

Some edge-case options and environments still need more coverage. Keep a
current backup before using preview builds on important repositories.

## Speed Snapshot
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "README.md",
            HunkFormatOptions::default(),
        )
        .expect("write readme hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            concat!(
                "@@ -1,15 +1,25 @@\n",
                " tuning through `ZMIN_GIT_HTTP_VERSION` is tracked as a Zmin-only environment\n",
                " control.\n",
                " \n",
                "-The staged-file hook runner is planned as a Zmin-only extension, with an API\n",
                "-shape like `zmin hooks run pre-commit --staged -- command ...`. It will be\n",
                "+The staged-file hook runner is available as a Zmin-only extension, including\n",
                "+preview, extension-filtered execution, and dry-run command surfaces. It is\n",
                " tracked below the extension inventory, not in the Git compatibility matrix.\n",
                " \n",
                " ## Preview Limits\n",
                " \n",
                " Zmin works with regular Git repositories and existing Git remotes. This preview\n",
                "-does not include Git LFS, reftable repositories or official package-manager\n",
                "-installs.\n",
                "+does not claim full Git LFS product parity, reftable repositories, or official\n",
                "+package-manager installs. Basic Git-LFS-style pointer workflows through\n",
                "+configured `filter.lfs.process` are verified for `add`, `checkout`, and\n",
                "+`cat-file --filters`. Built-in local `git lfs` foundation coverage is also\n",
                "+verified for `version`, `env`, `install --local --skip-repo`,\n",
                "+`install --local --skip-smudge`, `track`, `untrack`, `ls-files`, and local\n",
                "+`pre-push` stdin-shape validation. Network LFS transfer flows, batch API/auth,\n",
                "+and broader `git lfs ...` product parity remain out of scope. Repo-configured\n",
                "+`credential.helper=store` and\n",
                "+`credential.helper=cache` flows are verified for `git credential\n",
                "+fill|approve|reject`, but broader authenticated enterprise transport scenarios\n",
                "+still need dedicated gates.\n",
                " \n",
                " Some edge-case options and environments still need more coverage. Keep a\n",
                " current backup before using preview builds on important repositories.\n",
            )
        );
    }

    #[test]
    fn normalize_equal_run_rebinds_entire_equal_epilogue_block() {
        let ok = &b"    Ok(())\n"[..];
        let brace = &b"}\n"[..];
        let blank = &b"\n"[..];
        let header = &b"fn managed_hooks_add_staged_runner(\n"[..];
        let body = &b"    write_managed_hook_file(repo, hook_name, &[])?;\n"[..];
        let context = &b"fn managed_hooks_list() -> Result<()> {\n"[..];
        let mut ops = vec![
            DiffLineOp::Insert(b"    let commands = managed_hook_commands(repo, hook_name)?;\n"),
            DiffLineOp::Insert(b"    write_managed_hook_file(repo, hook_name, &commands)?;\n"),
            DiffLineOp::Equal(ok),
            DiffLineOp::Equal(brace),
            DiffLineOp::Equal(blank),
            DiffLineOp::Insert(header),
            DiffLineOp::Insert(body),
            DiffLineOp::Insert(ok),
            DiffLineOp::Insert(brace),
            DiffLineOp::Insert(blank),
            DiffLineOp::Equal(context),
        ];

        normalize_diff_equal_run_to_later_insert_duplicate_in_place(&mut ops);

        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (
                    b'+',
                    b"    let commands = managed_hook_commands(repo, hook_name)?;\n".to_vec(),
                ),
                (
                    b'+',
                    b"    write_managed_hook_file(repo, hook_name, &commands)?;\n".to_vec(),
                ),
                (b' ', ok.to_vec()),
                (b' ', brace.to_vec()),
                (b' ', blank.to_vec()),
                (b'+', header.to_vec()),
                (b'+', body.to_vec()),
                (b'+', ok.to_vec()),
                (b'+', brace.to_vec()),
                (b'+', blank.to_vec()),
                (b' ', context.to_vec()),
            ]
        );
    }

    #[test]
    fn normalize_equal_run_does_not_rebind_distant_matching_suffix() {
        let ok = &b"    Ok(())\n"[..];
        let brace = &b"}\n"[..];
        let blank = &b"\n"[..];
        let next_header = &b"fn reject_unmanaged_hook_file(repo: &GitRepo, hook_name: &str, force: bool) -> Result<()> {\n"[..];
        let mut ops = vec![
            DiffLineOp::Equal(ok),
            DiffLineOp::Equal(brace),
            DiffLineOp::Equal(blank),
            DiffLineOp::Insert(b"fn managed_hook_runner_value(repo: &GitRepo, hook_name: &str) -> Result<Option<String>> {\n"),
            DiffLineOp::Insert(b"    Ok(read_config_entries(repo)?\n"),
            DiffLineOp::Insert(b"        .into_iter()\n"),
            DiffLineOp::Insert(b"        .rev()\n"),
            DiffLineOp::Insert(b"        .find(|entry| true)\n"),
            DiffLineOp::Insert(b"        .map(|entry| entry.value))\n"),
            DiffLineOp::Insert(brace),
            DiffLineOp::Insert(blank),
            DiffLineOp::Insert(b"fn managed_hook_list_entries() -> Vec<&'static str> {\n"),
            DiffLineOp::Insert(b"    vec![\"a\"]\n"),
            DiffLineOp::Insert(b"        .into_iter()\n"),
            DiffLineOp::Insert(b"        .collect())\n"),
            DiffLineOp::Insert(brace),
            DiffLineOp::Insert(blank),
            DiffLineOp::Equal(next_header),
        ];

        normalize_diff_equal_run_to_later_insert_duplicate_in_place(&mut ops);

        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (b' ', ok.to_vec()),
                (b' ', brace.to_vec()),
                (b' ', blank.to_vec()),
                (
                    b'+',
                    b"fn managed_hook_runner_value(repo: &GitRepo, hook_name: &str) -> Result<Option<String>> {\n"
                        .to_vec(),
                ),
                (b'+', b"    Ok(read_config_entries(repo)?\n".to_vec()),
                (b'+', b"        .into_iter()\n".to_vec()),
                (b'+', b"        .rev()\n".to_vec()),
                (b'+', b"        .find(|entry| true)\n".to_vec()),
                (b'+', b"        .map(|entry| entry.value))\n".to_vec()),
                (b'+', brace.to_vec()),
                (b'+', blank.to_vec()),
                (b'+', b"fn managed_hook_list_entries() -> Vec<&'static str> {\n".to_vec()),
                (b'+', b"    vec![\"a\"]\n".to_vec()),
                (b'+', b"        .into_iter()\n".to_vec()),
                (b'+', b"        .collect())\n".to_vec()),
                (b'+', brace.to_vec()),
                (b'+', blank.to_vec()),
                (b' ', next_header.to_vec()),
            ]
        );
    }

    #[test]
    fn normalize_equal_run_does_not_rebind_distant_partial_suffix() {
        let collect = &b"        .collect())\n"[..];
        let brace = &b"}\n"[..];
        let blank = &b"\n"[..];
        let next_header = &b"fn reject_unmanaged_hook_file(repo: &GitRepo, hook_name: &str, force: bool) -> Result<()> {\n"[..];
        let mut ops = vec![
            DiffLineOp::Equal(b"    Ok(read_config_entries(repo)?\n"),
            DiffLineOp::Equal(b"        .into_iter()\n"),
            DiffLineOp::Equal(b"        .filter(|entry| managed_hook_entry_is_supported(entry) && entry.key == hook_name)\n"),
            DiffLineOp::Equal(b"        .map(|entry| entry.value)\n"),
            DiffLineOp::Equal(collect),
            DiffLineOp::Equal(brace),
            DiffLineOp::Equal(blank),
            DiffLineOp::Insert(b"fn managed_hook_runner_value(repo: &GitRepo, hook_name: &str) -> Result<Option<String>> {\n"),
            DiffLineOp::Insert(b"    Ok(read_config_entries(repo)?\n"),
            DiffLineOp::Insert(b"        .into_iter()\n"),
            DiffLineOp::Insert(b"        .rev()\n"),
            DiffLineOp::Insert(b"        .find(|entry| true)\n"),
            DiffLineOp::Insert(b"        .map(|entry| entry.value))\n"),
            DiffLineOp::Insert(b"}\n"),
            DiffLineOp::Insert(b"\n"),
            DiffLineOp::Insert(b"struct ManagedHookStagedEntries;\n"),
            DiffLineOp::Insert(b"impl ManagedHookStagedEntries {\n"),
            DiffLineOp::Insert(b"    fn render(&self) -> String {\n"),
            DiffLineOp::Insert(b"        vec![\"a\"]\n"),
            DiffLineOp::Insert(b"            .into_iter()\n"),
            DiffLineOp::Insert(b"            .map(|value| value)\n"),
        ];
        for _ in 0..40 {
            ops.push(DiffLineOp::Insert(b"        filler_line();\n"));
        }
        ops.extend([
            DiffLineOp::Insert(collect),
            DiffLineOp::Insert(brace),
            DiffLineOp::Insert(blank),
            DiffLineOp::Equal(next_header),
        ]);

        normalize_diff_equal_run_to_later_insert_duplicate_in_place(&mut ops);

        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (b' ', b"    Ok(read_config_entries(repo)?\n".to_vec()),
                (b' ', b"        .into_iter()\n".to_vec()),
                (
                    b' ',
                    b"        .filter(|entry| managed_hook_entry_is_supported(entry) && entry.key == hook_name)\n"
                        .to_vec(),
                ),
                (b' ', b"        .map(|entry| entry.value)\n".to_vec()),
                (b' ', collect.to_vec()),
                (b' ', brace.to_vec()),
                (b' ', blank.to_vec()),
                (
                    b'+',
                    b"fn managed_hook_runner_value(repo: &GitRepo, hook_name: &str) -> Result<Option<String>> {\n"
                        .to_vec(),
                ),
                (b'+', b"    Ok(read_config_entries(repo)?\n".to_vec()),
                (b'+', b"        .into_iter()\n".to_vec()),
                (b'+', b"        .rev()\n".to_vec()),
                (b'+', b"        .find(|entry| true)\n".to_vec()),
                (b'+', b"        .map(|entry| entry.value))\n".to_vec()),
                (b'+', b"}\n".to_vec()),
                (b'+', b"\n".to_vec()),
                (b'+', b"struct ManagedHookStagedEntries;\n".to_vec()),
                (b'+', b"impl ManagedHookStagedEntries {\n".to_vec()),
                (b'+', b"    fn render(&self) -> String {\n".to_vec()),
                (b'+', b"        vec![\"a\"]\n".to_vec()),
                (b'+', b"            .into_iter()\n".to_vec()),
                (b'+', b"            .map(|value| value)\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', b"        filler_line();\n".to_vec()),
                (b'+', collect.to_vec()),
                (b'+', brace.to_vec()),
                (b'+', blank.to_vec()),
                (b' ', next_header.to_vec()),
            ]
        );
    }

    #[test]
    fn normalize_equal_run_rebinds_partial_suffix_to_later_duplicate() {
        let close_brace = &b"}\n"[..];
        let blank = &b"\n"[..];
        let cfg = &b"#[cfg(unix)]\n"[..];
        let header = &b"fn credential_cache_send_to_stream(\n"[..];
        let mut ops = vec![
            DiffLineOp::Equal(close_brace),
            DiffLineOp::Equal(blank),
            DiffLineOp::Equal(cfg),
            DiffLineOp::Insert(b"fn credential_cache_request(\n"),
            DiffLineOp::Insert(b"    socket: &std::path::Path,\n"),
            DiffLineOp::Insert(b") -> Result<String> {\n"),
            DiffLineOp::Insert(b"    Ok(String::new())\n"),
            DiffLineOp::Insert(close_brace),
            DiffLineOp::Insert(blank),
            DiffLineOp::Insert(cfg),
            DiffLineOp::Equal(header),
        ];

        normalize_diff_equal_run_to_later_insert_duplicate_in_place(&mut ops);

        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (b' ', close_brace.to_vec()),
                (b' ', blank.to_vec()),
                (b'+', cfg.to_vec()),
                (b'+', b"fn credential_cache_request(\n".to_vec()),
                (b'+', b"    socket: &std::path::Path,\n".to_vec()),
                (b'+', b") -> Result<String> {\n".to_vec()),
                (b'+', b"    Ok(String::new())\n".to_vec()),
                (b'+', close_brace.to_vec()),
                (b'+', blank.to_vec()),
                (b' ', cfg.to_vec()),
                (b' ', header.to_vec()),
            ]
        );
    }

    #[test]
    fn normalize_equal_run_rebinds_opener_suffix_with_long_equal_prefix() {
        let mut ops = vec![
            DiffLineOp::Delete(b"    println!(\"Importing from {} into {}\", options.depot_path, repo.root.display());\n"),
            DiffLineOp::Equal(b"                format!(\n"),
            DiffLineOp::Equal(b"                    \"Initial import\"\n"),
            DiffLineOp::Equal(b"                )\n"),
            DiffLineOp::Equal(b"    println!(\n"),
            DiffLineOp::Insert(b"        \"Importing from {} into {}\",\n"),
            DiffLineOp::Insert(b"        options.depot_path,\n"),
            DiffLineOp::Insert(b"        repo.root.display()\n"),
            DiffLineOp::Insert(b"    );\n"),
            DiffLineOp::Insert(b"    println!(\n"),
            DiffLineOp::Equal(b"        \"Doing initial import of {depot_root} from revision #head into {}\",\n"),
            DiffLineOp::Equal(b"        options.branch\n"),
            DiffLineOp::Equal(b"    );\n"),
        ];

        normalize_diff_equal_run_to_later_insert_duplicate_in_place(&mut ops);

        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (
                    b'-',
                    b"    println!(\"Importing from {} into {}\", options.depot_path, repo.root.display());\n"
                        .to_vec(),
                ),
                (b' ', b"                format!(\n".to_vec()),
                (b' ', b"                    \"Initial import\"\n".to_vec()),
                (b' ', b"                )\n".to_vec()),
                (b' ', b"    println!(\n".to_vec()),
                (b'+', b"        \"Importing from {} into {}\",\n".to_vec()),
                (b'+', b"        options.depot_path,\n".to_vec()),
                (b'+', b"        repo.root.display()\n".to_vec()),
                (b'+', b"    );\n".to_vec()),
                (b'+', b"    println!(\n".to_vec()),
                (
                    b' ',
                    b"        \"Doing initial import of {depot_root} from revision #head into {}\",\n"
                        .to_vec(),
                ),
                (b' ', b"        options.branch\n".to_vec()),
                (b' ', b"    );\n".to_vec()),
            ]
        );
    }

    #[test]
    fn normalize_equal_run_keeps_function_epilogue_as_context() {
        let ok = &b"    Ok(())\n"[..];
        let close_brace = &b"}\n"[..];
        let blank = &b"\n"[..];
        let next_header =
            &b"fn normalize_managed_hook_name(hook_name: &str) -> Result<&str> {\n"[..];
        let mut ops = vec![
            DiffLineOp::Equal(ok),
            DiffLineOp::Equal(close_brace),
            DiffLineOp::Equal(blank),
            DiffLineOp::Insert(
                b"fn unset_config_value_if_present(repo: &GitRepo, name: &str) -> Result<()> {\n",
            ),
            DiffLineOp::Insert(b"    if read_config_entry(repo, name)?.is_some() {\n"),
            DiffLineOp::Insert(b"        unset_config_value(repo, name)?;\n"),
            DiffLineOp::Insert(b"    }\n"),
            DiffLineOp::Insert(ok),
            DiffLineOp::Insert(close_brace),
            DiffLineOp::Insert(blank),
            DiffLineOp::Equal(next_header),
        ];

        normalize_diff_equal_run_to_later_insert_duplicate_in_place(&mut ops);

        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (b' ', ok.to_vec()),
                (b' ', close_brace.to_vec()),
                (b' ', blank.to_vec()),
                (
                    b'+',
                    b"fn unset_config_value_if_present(repo: &GitRepo, name: &str) -> Result<()> {\n"
                        .to_vec(),
                ),
                (b'+', b"    if read_config_entry(repo, name)?.is_some() {\n".to_vec()),
                (b'+', b"        unset_config_value(repo, name)?;\n".to_vec()),
                (b'+', b"    }\n".to_vec()),
                (b'+', ok.to_vec()),
                (b'+', close_brace.to_vec()),
                (b'+', blank.to_vec()),
                (b' ', next_header.to_vec()),
            ]
        );
    }

    #[test]
    fn unified_hunk_keeps_function_epilogue_context_before_inserted_helper() {
        let old = br#"fn managed_hooks_remove(hook_name: &str) -> Result<()> {
    let hook_path = repo.git_dir.join("hooks").join(hook_name);
    if hook_path.is_file() && managed_hook_file_is_owned(&hook_path)? {
        fs::remove_file(hook_path)?;
    }
    Ok(())
}

fn normalize_managed_hook_name(hook_name: &str) -> Result<&str> {
"#;
        let new = br#"fn managed_hooks_remove(hook_name: &str) -> Result<()> {
    let hook_path = repo.git_dir.join("hooks").join(hook_name);
    if hook_path.is_file() && managed_hook_file_is_owned(&hook_path)? {
        fs::remove_file(hook_path)?;
    }
    Ok(())
}

fn unset_config_value_if_present(repo: &GitRepo, name: &str) -> Result<()> {
    if read_config_entry(repo, name)?.is_some() {
        unset_config_value(repo, name)?;
    }
    Ok(())
}

fn normalize_managed_hook_name(hook_name: &str) -> Result<&str> {
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "admin_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write managed hook helper hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            concat!(
                "@@ -6,4 +6,11 @@ fn managed_hooks_remove(hook_name: &str) -> Result<()> {\n",
                "     Ok(())\n",
                " }\n",
                " \n",
                "+fn unset_config_value_if_present(repo: &GitRepo, name: &str) -> Result<()> {\n",
                "+    if read_config_entry(repo, name)?.is_some() {\n",
                "+        unset_config_value(repo, name)?;\n",
                "+    }\n",
                "+    Ok(())\n",
                "+}\n",
                "+\n",
                " fn normalize_managed_hook_name(hook_name: &str) -> Result<&str> {\n",
            )
        );
    }

    #[test]
    fn unified_hunk_keeps_method_chain_tail_as_context_before_inserted_helper() {
        let old =
            br#"fn managed_hook_commands(repo: &GitRepo, hook_name: &str) -> Result<Vec<String>> {
    Ok(read_config_entries(repo)?
        .into_iter()
        .filter(|entry| managed_hook_entry_is_supported(entry) && entry.key == hook_name)
        .map(|entry| entry.value)
        .collect())
}

fn reject_unmanaged_hook_file(repo: &GitRepo, hook_name: &str, force: bool) -> Result<()> {
"#;
        let new = br#"fn managed_hook_commands(repo: &GitRepo, hook_name: &str) -> Result<Vec<String>> {
    Ok(read_config_entries(repo)?
        .into_iter()
        .filter(|entry| managed_hook_entry_is_supported(entry) && entry.key == hook_name)
        .map(|entry| entry.value)
        .collect())
}

fn managed_hook_runner_value(repo: &GitRepo, hook_name: &str) -> Result<Option<String>> {
    Ok(read_config_entries(repo)?
        .into_iter()
        .rev()
        .find(|entry| entry.section == "zmin" && entry.subsection == "hooks-runner" && entry.key == hook_name)
        .map(|entry| entry.value))
}

fn reject_unmanaged_hook_file(repo: &GitRepo, hook_name: &str, force: bool) -> Result<()> {
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "admin_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write managed hook runner hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            concat!(
                "@@ -6,4 +6,12 @@ fn managed_hook_commands(repo: &GitRepo, hook_name: &str) -> Result<Vec<String>>\n",
                "         .collect())\n",
                " }\n",
                " \n",
                "+fn managed_hook_runner_value(repo: &GitRepo, hook_name: &str) -> Result<Option<String>> {\n",
                "+    Ok(read_config_entries(repo)?\n",
                "+        .into_iter()\n",
                "+        .rev()\n",
                "+        .find(|entry| entry.section == \"zmin\" && entry.subsection == \"hooks-runner\" && entry.key == hook_name)\n",
                "+        .map(|entry| entry.value))\n",
                "+}\n",
                "+\n",
                " fn reject_unmanaged_hook_file(repo: &GitRepo, hook_name: &str, force: bool) -> Result<()> {\n",
            )
        );
    }

    #[test]
    fn unified_hunk_keeps_multiline_call_opening_in_inserted_block() {
        let old =
            br#"    println!("Importing from {} into {}", options.depot_path, repo.root.display());
    println!(
        "Doing initial import of {depot_root} from revision #head into {}",
        options.branch
    );
"#;
        let new = br#"    println!(
        "Importing from {} into {}",
        options.depot_path,
        repo.root.display()
    );
    println!(
        "Doing initial import of {depot_root} from revision #head into {}",
        options.branch
    );
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "admin_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write multiline call wrapping hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            concat!(
                "@@ -1,4 +1,8 @@\n",
                "-    println!(\"Importing from {} into {}\", options.depot_path, repo.root.display());\n",
                "+    println!(\n",
                "+        \"Importing from {} into {}\",\n",
                "+        options.depot_path,\n",
                "+        repo.root.display()\n",
                "+    );\n",
                "     println!(\n",
                "         \"Doing initial import of {depot_root} from revision #head into {}\",\n",
                "         options.branch\n",
            )
        );
    }

    #[test]
    fn unified_hunk_keeps_multiline_call_opening_in_inserted_block_on_myers_path() {
        let mut old = String::new();
        let mut new = String::new();
        for idx in 0..40 {
            old.push_str(&format!("fn filler_before_{idx}() {{}}\n"));
            new.push_str(&format!("fn filler_before_{idx}() {{}}\n"));
        }
        old.push_str(
            "    println!(\"Importing from {} into {}\", options.depot_path, repo.root.display());\n",
        );
        old.push_str("    println!(\n");
        old.push_str(
            "        \"Doing initial import of {depot_root} from revision #head into {}\",\n",
        );
        old.push_str("        options.branch\n");
        old.push_str("    );\n");
        new.push_str("    println!(\n");
        new.push_str("        \"Importing from {} into {}\",\n");
        new.push_str("        options.depot_path,\n");
        new.push_str("        repo.root.display()\n");
        new.push_str("    );\n");
        new.push_str("    println!(\n");
        new.push_str(
            "        \"Doing initial import of {depot_root} from revision #head into {}\",\n",
        );
        new.push_str("        options.branch\n");
        new.push_str("    );\n");
        for idx in 0..40 {
            old.push_str(&format!("fn filler_after_{idx}() {{}}\n"));
            new.push_str(&format!("fn filler_after_{idx}() {{}}\n"));
        }

        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old.as_bytes(),
            new.as_bytes(),
            "admin_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write multiline call wrapping hunk on myers path");

        let rendered = String::from_utf8(out).expect("utf8");
        assert!(rendered.contains(
            "-    println!(\"Importing from {} into {}\", options.depot_path, repo.root.display());\n+    println!(\n+        \"Importing from {} into {}\",\n+        options.depot_path,\n+        repo.root.display()\n+    );\n     println!(\n"
        ));
        assert!(!rendered.contains(
            "-    println!(\"Importing from {} into {}\", options.depot_path, repo.root.display());\n     println!(\n+        \"Importing from {} into {}\",\n"
        ));
    }

    #[test]
    fn unified_hunk_matches_stock_shape_for_real_p4_clone_multiline_print() {
        let old = br#"    let id = store.write_object(
        GitObjectKind::Commit,
        &CommitBuilder::new(tree, signature.clone(), signature)
            .message(
                format!(
                    "Initial import of {depot_root} from the state at revision #head\n\n[git-p4: depot-paths = \"{depot_root}\": change = {latest_change}]\n"
                )
                .into_bytes(),
            )?
            .encode()?,
    )?;
    let local_head = format!("refs/heads/{initial_branch}");
    refs.write_ref(&local_head, &id)?;
    refs.write_ref(&options.branch, &id)?;
    refs.write_ref("refs/remotes/p4/HEAD", &id)?;
    refs.write_symbolic_ref("HEAD", &local_head)?;
    println!(
        "Initialized empty Git repository in {}",
        display_path_with_trailing_separator(&repo.git_dir)
    );
    println!("Importing from {} into {}", options.depot_path, repo.root.display());
    println!(
        "Doing initial import of {depot_root} from revision #head into {}",
        options.branch
    );
    Ok(())
}

fn emit_p4_clone_usage_stdout() {
    print!(
        concat!(
            "Usage: git-p4 clone [options] //depot/path[@revRange]\n\n",
            "Creates a new git repository and imports from Perforce into it\n\n",
            "Options:\n",
            "  --branch=BRANCH       \n",
"#;
        let new = br#"                format!(
                    "Initial import of {depot_root} from the state at revision #head\n\n[git-p4: depot-paths = \"{depot_root}\": change = {latest_change}]\n"
                )
                .into_bytes(),
            )?
            .encode()?,
    )?;
    let local_head = format!("refs/heads/{initial_branch}");
    refs.write_ref(&local_head, &id)?;
    refs.write_ref(&options.branch, &id)?;
    refs.write_ref("refs/remotes/p4/HEAD", &id)?;
    refs.write_symbolic_ref("HEAD", &local_head)?;
    println!(
        "Initialized empty Git repository in {}",
        display_path_with_trailing_separator(&repo.git_dir)
    );
    println!(
        "Importing from {} into {}",
        options.depot_path,
        repo.root.display()
    );
    println!(
        "Doing initial import of {depot_root} from revision #head into {}",
        options.branch
    );
    Ok(())
}

fn emit_p4_clone_usage_stdout() {
    print!(concat!(
        "Usage: git-p4 clone [options] //depot/path[@revRange]\n\n",
        "Creates a new git repository and imports from Perforce into it\n\n",
        "Options:\n",
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "admin_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write real p4 clone hunk");

        let rendered = String::from_utf8(out).expect("utf8");
        assert!(rendered.contains("-    println!(\"Importing from {} into {}\", options.depot_path, repo.root.display());\n+    println!(\n+        \"Importing from {} into {}\",\n+        options.depot_path,\n+        repo.root.display()\n+    );\n     println!(\n"));
        assert!(!rendered.contains("-    println!(\"Importing from {} into {}\", options.depot_path, repo.root.display());\n     println!(\n+        \"Importing from {} into {}\",\n"));
    }

    #[test]
    fn unified_hunk_matches_stock_shape_for_credential_helper_insertion_epilogue() {
        let old = br#"fn credential_fill(entries: Vec<(String, String)>) -> Result<()> {
    let username = credential_value(&entries, "username");
    let password = credential_value(&entries, "password");
    let protocol = credential_value(&entries, "protocol").unwrap_or("");
    let host = credential_value(&entries, "host").unwrap_or("");
    match (username, password) {
        (Some(_), Some(_)) => {
            for (key, value) in entries {
                println!("{key}={value}");
            }
            Ok(())
        }
        (None, _) => Err(CliError::Fatal {
            code: 128,
            message: format!(
                "could not read Username for '{}': Device not configured",
                credential_url(protocol, None, host)
            ),
        }),
        (Some(username), None) => Err(CliError::Fatal {
            code: 128,
            message: format!(
                "could not read Password for '{}': Device not configured",
                credential_url(protocol, Some(username), host)
            ),
        }),
    }
}

fn credential_value<'a>(entries: &'a [(String, String)], key: &str) -> Option<&'a str> {
"#;
        let new = br#"fn credential_fill(
    entries: Vec<(String, String)>,
    helpers: &[ConfiguredCredentialHelper],
) -> Result<()> {
    let username = credential_value(&entries, "username");
    let password = credential_value(&entries, "password");
    let protocol = credential_value(&entries, "protocol").unwrap_or("");
    let host = credential_value(&entries, "host").unwrap_or("");
    match (username, password) {
        (Some(_), Some(_)) => {
            for (key, value) in &entries {
                println!("{key}={value}");
            }
            Ok(())
        }
        _ => {
            if let Some(filled) = credential_fill_from_helpers(&entries, helpers)? {
                for (key, value) in filled {
                    println!("{key}={value}");
                }
                return Ok(());
            }
            match (username, password) {
                (None, _) => Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "could not read Username for '{}': Device not configured",
                        credential_url(protocol, None, host)
                    ),
                }),
                (Some(username), None) => Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "could not read Password for '{}': Device not configured",
                        credential_url(protocol, Some(username), host)
                    ),
                }),
                _ => unreachable!(),
            }
        }
    }
}

fn credential_fill_from_helpers(
    entries: &[(String, String)],
    helpers: &[ConfiguredCredentialHelper],
) -> Result<Option<Vec<(String, String)>>> {
    for helper in helpers {
        let resolved = match helper {
            ConfiguredCredentialHelper::Store { file } => {
                credential_fill_from_store_helper(entries, file.clone())?
            }
            ConfiguredCredentialHelper::Cache { timeout, socket } => {
                credential_fill_from_cache_helper(entries, *timeout, socket.clone())?
            }
        };
        if let Some(resolved) = resolved {
            let mut filled = entries.to_vec();
            set_credential_entry(&mut filled, "username", &resolved.username);
            set_credential_entry(&mut filled, "password", &resolved.password);
            return Ok(Some(filled));
        }
    }
    Ok(None)
}

fn credential_fill_from_store_helper(
    entries: &[(String, String)],
    file: Option<PathBuf>,
) -> Result<Option<ResolvedCredential>> {
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "credential_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write credential helper insertion hunk");

        let rendered = String::from_utf8(out).expect("utf8");
        assert!(
            rendered.contains("+    Ok(None)\n+}\n+\n+fn credential_fill_from_store_helper(\n")
        );
        assert!(
            !rendered.contains("+    Ok(None)\n }\n \n+fn credential_fill_from_store_helper(\n")
        );
    }

    #[test]
    fn unified_hunk_matches_stock_shape_for_credential_helper_full_context_tail() {
        let old = br#"fn credential_fill(entries: Vec<(String, String)>) -> Result<()> {
    let username = credential_value(&entries, "username");
    let password = credential_value(&entries, "password");
    let protocol = credential_value(&entries, "protocol").unwrap_or("");
    let host = credential_value(&entries, "host").unwrap_or("");
    match (username, password) {
        (Some(_), Some(_)) => {
            for (key, value) in entries {
                println!("{key}={value}");
            }
            Ok(())
        }
        (None, _) => Err(CliError::Fatal {
            code: 128,
            message: format!(
                "could not read Username for '{}': Device not configured",
                credential_url(protocol, None, host)
            ),
        }),
        (Some(username), None) => Err(CliError::Fatal {
            code: 128,
            message: format!(
                "could not read Password for '{}': Device not configured",
                credential_url(protocol, Some(username), host)
            ),
        }),
    }
}

fn credential_value<'a>(entries: &'a [(String, String)], key: &str) -> Option<&'a str> {
    entries
        .iter()
        .rev()
        .find_map(|(entry_key, value)| (entry_key == key).then_some(value.as_str()))
}
"#;
        let new = br#"fn credential_fill(
    entries: Vec<(String, String)>,
    helpers: &[ConfiguredCredentialHelper],
) -> Result<()> {
    let username = credential_value(&entries, "username");
    let password = credential_value(&entries, "password");
    let protocol = credential_value(&entries, "protocol").unwrap_or("");
    let host = credential_value(&entries, "host").unwrap_or("");
    match (username, password) {
        (Some(_), Some(_)) => {
            for (key, value) in &entries {
                println!("{key}={value}");
            }
            Ok(())
        }
        _ => {
            if let Some(filled) = credential_fill_from_helpers(&entries, helpers)? {
                for (key, value) in filled {
                    println!("{key}={value}");
                }
                return Ok(());
            }
            match (username, password) {
                (None, _) => Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "could not read Username for '{}': Device not configured",
                        credential_url(protocol, None, host)
                    ),
                }),
                (Some(username), None) => Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "could not read Password for '{}': Device not configured",
                        credential_url(protocol, Some(username), host)
                    ),
                }),
                _ => unreachable!(),
            }
        }
    }
}

fn credential_fill_from_helpers(
    entries: &[(String, String)],
    helpers: &[ConfiguredCredentialHelper],
) -> Result<Option<Vec<(String, String)>>> {
    for helper in helpers {
        let resolved = match helper {
            ConfiguredCredentialHelper::Store { file } => {
                credential_fill_from_store_helper(entries, file.clone())?
            }
            ConfiguredCredentialHelper::Cache { timeout, socket } => {
                credential_fill_from_cache_helper(entries, *timeout, socket.clone())?
            }
        };
        if let Some(resolved) = resolved {
            let mut filled = entries.to_vec();
            set_credential_entry(&mut filled, "username", &resolved.username);
            set_credential_entry(&mut filled, "password", &resolved.password);
            return Ok(Some(filled));
        }
    }
    Ok(None)
}

fn credential_fill_from_store_helper(
    entries: &[(String, String)],
    file: Option<PathBuf>,
) -> Result<Option<ResolvedCredential>> {
    let path = credential_store_path(file)?;
    let rows = read_credential_store_rows(&path)?;
    for row in rows.iter().rev() {
        if credential_store_row_matches(row, entries) {
            return Ok(Some(ResolvedCredential {
                username: row.username.clone(),
                password: row.password.clone(),
            }));
        }
    }
    Ok(None)
}

fn credential_fill_from_cache_helper(
    entries: &[(String, String)],
    timeout: Option<u64>,
    socket: Option<PathBuf>,
) -> Result<Option<ResolvedCredential>> {
    let socket = credential_cache_socket_path(socket)?;
    #[cfg(unix)]
    {
        let response = credential_cache_request(&socket, timeout, "get", entries)?;
        let resolved = parse_credential_entries(&response)?;
        let Some(username) = credential_value(&resolved, "username") else {
            return Ok(None);
        };
        let Some(password) = credential_value(&resolved, "password") else {
            return Ok(None);
        };
        return Ok(Some(ResolvedCredential {
            username: username.to_owned(),
            password: password.to_owned(),
        }));
    }
    #[cfg(not(unix))]
    {
        let _ = (timeout, socket, entries);
        Ok(None)
    }
}

fn credential_approve_or_reject(
    entries: Vec<(String, String)>,
    helpers: &[ConfiguredCredentialHelper],
    action: &str,
) -> Result<()> {
    for helper in helpers {
        match helper {
            ConfiguredCredentialHelper::Store { file } => {
                let path = credential_store_path(file.clone())?;
                match action {
                    "store" => credential_store_store(&path, &entries)?,
                    "erase" => credential_store_erase(&path, &entries)?,
                    _ => {}
                }
            }
            ConfiguredCredentialHelper::Cache { timeout, socket } => {
                let socket = credential_cache_socket_path(socket.clone())?;
                #[cfg(unix)]
                {
                    let _ = credential_cache_request(&socket, *timeout, action, &entries)?;
                }
                #[cfg(not(unix))]
                {
                    let _ = (timeout, socket);
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedCredential {
    username: String,
    password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConfiguredCredentialHelper {
    Store {
        file: Option<PathBuf>,
    },
    Cache {
        timeout: Option<u64>,
        socket: Option<PathBuf>,
    },
}

fn configured_credential_helpers() -> Vec<ConfiguredCredentialHelper> {
    let Ok(repo) = find_repo() else {
        return Vec::new();
    };
    let Ok(entries) = read_config_entries(&repo) else {
        return Vec::new();
    };
    let mut helpers = Vec::new();
    for entry in entries {
        if entry.section != "credential" || !entry.subsection.is_empty() || entry.key != "helper" {
            continue;
        }
        if entry.value.is_empty() {
            helpers.clear();
            continue;
        }
        if let Ok(Some(helper)) = parse_configured_credential_helper(&entry.value) {
            helpers.push(helper);
        }
    }
    helpers
}

fn parse_configured_credential_helper(value: &str) -> Result<Option<ConfiguredCredentialHelper>> {
    let words = transport_commands::split_shell_words(value)?;
    let Some(name) = words.first().map(String::as_str) else {
        return Ok(None);
    };
    match name {
        "store" => {
            let mut file = None;
            let mut parts = words.into_iter().skip(1);
            while let Some(part) = parts.next() {
                if part == "--file" {
                    let Some(path) = parts.next() else {
                        return Err(CliError::Fatal {
                            code: 128,
                            message: "credential-store helper is missing --file path".into(),
                        });
                    };
                    file = Some(PathBuf::from(path));
                } else if let Some(path) = part.strip_prefix("--file=") {
                    file = Some(PathBuf::from(path));
                }
            }
            Ok(Some(ConfiguredCredentialHelper::Store { file }))
        }
        "cache" => {
            let mut timeout = None;
            let mut socket = None;
            let mut parts = words.into_iter().skip(1);
            while let Some(part) = parts.next() {
                if part == "--timeout" {
                    let Some(value) = parts.next() else {
                        return Err(CliError::Fatal {
                            code: 128,
                            message: "credential-cache helper is missing --timeout value".into(),
                        });
                    };
                    timeout = Some(parse_credential_cache_timeout_value(&value)?);
                } else if let Some(value) = part.strip_prefix("--timeout=") {
                    timeout = Some(parse_credential_cache_timeout_value(value)?);
                } else if part == "--socket" {
                    let Some(value) = parts.next() else {
                        return Err(CliError::Fatal {
                            code: 128,
                            message: "credential-cache helper is missing --socket path".into(),
                        });
                    };
                    socket = Some(PathBuf::from(value));
                } else if let Some(value) = part.strip_prefix("--socket=") {
                    socket = Some(PathBuf::from(value));
                }
            }
            Ok(Some(ConfiguredCredentialHelper::Cache { timeout, socket }))
        }
        _ => Ok(None),
    }
}

fn parse_credential_cache_timeout_value(value: &str) -> Result<u64> {
    value.parse::<u64>().map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("credential-cache helper has invalid timeout '{value}'"),
    })
}

fn set_credential_entry(entries: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some((_, existing)) = entries.iter_mut().find(|(entry_key, _)| entry_key == key) {
        *existing = value.to_owned();
    } else {
        entries.push((key.to_owned(), value.to_owned()));
    }
}

fn credential_value<'a>(entries: &'a [(String, String)], key: &str) -> Option<&'a str> {
    entries
        .iter()
        .rev()
        .find_map(|(entry_key, value)| (entry_key == key).then_some(value.as_str()))
}
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "credential_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write credential helper full-context hunk");

        let rendered = String::from_utf8(out).expect("utf8");
        assert!(
            rendered
                .contains("+    }\n+    Ok(None)\n+}\n+\n+fn credential_fill_from_store_helper(\n")
        );
        assert!(
            !rendered
                .contains("     }\n+    Ok(None)\n }\n \n+fn credential_fill_from_store_helper(\n")
        );
        assert!(rendered.starts_with("@@ -1,"));
    }

    #[test]
    #[ignore = "debug exact command_with_stdin eof-tail hunk shape"]
    fn debug_command_with_stdin_eof_tail_hunk() {
        let old = br#"    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}
"#;
        let new = br#"    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let (code, stdout, stderr) =
        command_status_with_home_cwd_stdin(command, home, cwd, args, stdin, label);
    assert_eq!(code, 0, "{label} failed: {stderr}");
    stdout
}

fn command_status_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout)
            .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .unwrap_or_else(|err| panic!("{label} stderr utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
    )
}
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "git_credential_compat.rs",
            HunkFormatOptions::default(),
        )
        .expect("write command_with_stdin eof tail hunk");
        panic!("{}", String::from_utf8(out).expect("utf8"));
    }

    #[test]
    #[ignore = "debug larger credential tail alignment against new helper block"]
    fn debug_credential_tail_alignment_hunk() {
        let old = br#"    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}
"#;
        let new = br#"fn command_with_home_stdin(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let (code, stdout, stderr) =
        command_status_with_home_cwd_stdin(command, home, cwd, args, stdin, label);
    assert_eq!(code, 0, "{label} failed: {stderr}");
    stdout
}

fn command_status_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout)
            .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .unwrap_or_else(|err| panic!("{label} stderr utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
    )
}
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "git_credential_compat.rs",
            HunkFormatOptions::default(),
        )
        .expect("write larger credential tail hunk");
        panic!("{}", String::from_utf8(out).expect("utf8"));
    }

    #[test]
    #[ignore = "debug exact command_with_home_stdin + command_with_stdin full fragment"]
    fn debug_command_with_home_and_stdin_fragment_hunk() {
        let old = br#"fn command_with_home_stdin(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}
"#;
        let new = br#"fn command_with_home_stdin(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let (code, stdout, stderr) =
        command_status_with_home_cwd_stdin(command, home, cwd, args, stdin, label);
    assert_eq!(code, 0, "{label} failed: {stderr}");
    stdout
}

fn command_status_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout)
            .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .unwrap_or_else(|err| panic!("{label} stderr utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
    )
}
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "git_credential_compat.rs",
            HunkFormatOptions::default(),
        )
        .expect("write command_with_home_stdin fragment hunk");
        panic!("{}", String::from_utf8(out).expect("utf8"));
    }

    #[test]
    #[ignore = "debug exact credential fragment with configure_helper before tail"]
    fn debug_configure_helper_tail_fragment_hunk() {
        let old = br#"    command_stdout_bytes(
        "git",
        dir.path(),
        &["credential-cache", git_socket_arg.as_str(), "exit"],
    );
}

fn command_with_home_stdin(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}
"#;
        let new = br#"    command_stdout_bytes(
        "git",
        dir.path(),
        &[
            "credential-cache",
            &format!("--socket={}", git_socket.display()),
            "exit",
        ],
    );
}

fn configure_helper(repo: &std::path::Path, helper: &str) {
    crate::stock_git_support::git_config_add(repo, "credential.helper", helper);
}

fn command_with_home_stdin(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let (code, stdout, stderr) =
        command_status_with_home_cwd_stdin(command, home, cwd, args, stdin, label);
    assert_eq!(code, 0, "{label} failed: {stderr}");
    stdout
}

fn command_status_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout)
            .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .unwrap_or_else(|err| panic!("{label} stderr utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
    )
}
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "git_credential_compat.rs",
            HunkFormatOptions::default(),
        )
        .expect("write configure_helper tail fragment hunk");
        panic!("{}", String::from_utf8(out).expect("utf8"));
    }

    #[test]
    fn unified_hunk_rebinds_eof_closing_brace_before_inserted_helper_block() {
        let old = br#"    command_stdout_bytes(
        "git",
        dir.path(),
        &["credential-cache", git_socket_arg.as_str(), "exit"],
    );
}

fn command_with_home_stdin(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}
"#;
        let new = br#"    command_stdout_bytes(
        "git",
        dir.path(),
        &[
            "credential-cache",
            &format!("--socket={}", git_socket.display()),
            "exit",
        ],
    );
}

fn configure_helper(repo: &std::path::Path, helper: &str) {
    crate::stock_git_support::git_config_add(repo, "credential.helper", helper);
}

fn command_with_home_stdin(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let (code, stdout, stderr) =
        command_status_with_home_cwd_stdin(command, home, cwd, args, stdin, label);
    assert_eq!(code, 0, "{label} failed: {stderr}");
    stdout
}

fn command_status_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout)
            .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .unwrap_or_else(|err| panic!("{label} stderr utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
    )
}
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "git_credential_compat.rs",
            HunkFormatOptions::default(),
        )
        .expect("write rebind eof closing brace hunk");

        let rendered = String::from_utf8(out).expect("utf8");
        assert!(
            rendered.contains(concat!(
                "@@ -72,3 +80,57 @@ fn command_with_stdin(\n",
                "         .trim_end_matches('\\n')\n",
                "         .to_owned()\n",
                " }\n",
                "+\n",
                "+fn command_with_home_cwd_stdin(\n",
            )),
            "{}",
            rendered
        );
        assert!(
            !rendered.contains(".to_owned()\n+}\n+\n+fn command_with_home_cwd_stdin("),
            "{}",
            rendered
        );
    }

    #[test]
    #[ignore = "debug ops for configure_helper tail fragment"]
    fn debug_configure_helper_tail_fragment_ops() {
        let old = br#"    command_stdout_bytes(
        "git",
        dir.path(),
        &["credential-cache", git_socket_arg.as_str(), "exit"],
    );
}

fn command_with_home_stdin(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}
"#;
        let new = br#"    command_stdout_bytes(
        "git",
        dir.path(),
        &[
            "credential-cache",
            &format!("--socket={}", git_socket.display()),
            "exit",
        ],
    );
}

fn configure_helper(repo: &std::path::Path, helper: &str) {
    crate::stock_git_support::git_config_add(repo, "credential.helper", helper);
}

fn command_with_home_stdin(
    command: &str,
    home: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {command} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {command}: {err}"));
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
        .trim_end_matches('\n')
        .to_owned()
}

fn command_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> String {
    let (code, stdout, stderr) =
        command_status_with_home_cwd_stdin(command, home, cwd, args, stdin, label);
    assert_eq!(code, 0, "{label} failed: {stderr}");
    stdout
}

fn command_status_with_home_cwd_stdin(
    command: &str,
    home: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    stdin: &str,
    label: &str,
) -> (i32, String, String) {
    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .unwrap_or_else(|err| panic!("write {label} stdin: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout)
            .unwrap_or_else(|err| panic!("{label} stdout utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .unwrap_or_else(|err| panic!("{label} stderr utf8: {err}"))
            .trim_end_matches('\n')
            .to_owned(),
    )
}
"#;
        panic!("{:?}", debug_line_tags(old, new));
    }

    #[test]
    fn unified_hunk_matches_stock_shape_for_adjacent_helper_insertions() {
        let old = br#"fn managed_hook_config_key(hook_name: &str) -> String {
    format!("zmin.hooks.{hook_name}")
}

fn managed_hook_entry_is_supported(entry: &ConfigEntry) -> bool {
    entry.section == "zmin"
        && entry.subsection == "hooks"
        && managed_hook_name_is_supported(&entry.key)
}

fn managed_hook_commands(repo: &GitRepo, hook_name: &str) -> Result<Vec<String>> {
    Ok(read_config_entries(repo)?
        .into_iter()
        .filter(|entry| managed_hook_entry_is_supported(entry) && entry.key == hook_name)
        .map(|entry| entry.value)
        .collect())
}

fn reject_unmanaged_hook_file(repo: &GitRepo, hook_name: &str, force: bool) -> Result<()> {
"#;
        let new = br#"fn managed_hook_config_key(hook_name: &str) -> String {
    format!("zmin.hooks.{hook_name}")
}

fn managed_hook_runner_config_key(hook_name: &str) -> String {
    format!("zmin.hooks-runner.{hook_name}")
}

fn managed_hook_entry_is_supported(entry: &ConfigEntry) -> bool {
    entry.section == "zmin"
        && entry.subsection == "hooks"
        && managed_hook_name_is_supported(&entry.key)
}

fn managed_hook_commands(repo: &GitRepo, hook_name: &str) -> Result<Vec<String>> {
    Ok(read_config_entries(repo)?
        .into_iter()
        .filter(|entry| managed_hook_entry_is_supported(entry) && entry.key == hook_name)
        .map(|entry| entry.value)
        .collect())
}

fn managed_hook_runner_value(repo: &GitRepo, hook_name: &str) -> Result<Option<String>> {
    Ok(read_config_entries(repo)?
        .into_iter()
        .rev()
        .find(|entry| entry.section == "zmin" && entry.subsection == "hooks-runner" && entry.key == hook_name)
        .map(|entry| entry.value))
}

fn reject_unmanaged_hook_file(repo: &GitRepo, hook_name: &str, force: bool) -> Result<()> {
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "admin_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write adjacent helper insertion hunks");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            concat!(
                "@@ -2,6 +2,10 @@ fn managed_hook_config_key(hook_name: &str) -> String {\n",
                "     format!(\"zmin.hooks.{hook_name}\")\n",
                " }\n",
                " \n",
                "+fn managed_hook_runner_config_key(hook_name: &str) -> String {\n",
                "+    format!(\"zmin.hooks-runner.{hook_name}\")\n",
                "+}\n",
                "+\n",
                " fn managed_hook_entry_is_supported(entry: &ConfigEntry) -> bool {\n",
                "     entry.section == \"zmin\"\n",
                "         && entry.subsection == \"hooks\"\n",
                "@@ -16,4 +20,12 @@ fn managed_hook_commands(repo: &GitRepo, hook_name: &str) -> Result<Vec<String>>\n",
                "         .collect())\n",
                " }\n",
                " \n",
                "+fn managed_hook_runner_value(repo: &GitRepo, hook_name: &str) -> Result<Option<String>> {\n",
                "+    Ok(read_config_entries(repo)?\n",
                "+        .into_iter()\n",
                "+        .rev()\n",
                "+        .find(|entry| entry.section == \"zmin\" && entry.subsection == \"hooks-runner\" && entry.key == hook_name)\n",
                "+        .map(|entry| entry.value))\n",
                "+}\n",
                "+\n",
                " fn reject_unmanaged_hook_file(repo: &GitRepo, hook_name: &str, force: bool) -> Result<()> {\n",
            )
        );
    }

    #[test]
    fn normalize_change_run_keeps_structural_prefix_as_context() {
        let mut ops = vec![
            DiffLineOp::Equal(b"    Help {\n"),
            DiffLineOp::Equal(
                b"        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]\n",
            ),
            DiffLineOp::Equal(b"        args: Vec<String>,\n"),
            DiffLineOp::Delete(b"    },\n"),
            DiffLineOp::Insert(b"    },\n"),
            DiffLineOp::Insert(b"    Lfs {\n"),
            DiffLineOp::Insert(
                b"        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]\n",
            ),
            DiffLineOp::Insert(b"        args: Vec<String>,\n"),
            DiffLineOp::Insert(b"    },\n"),
            DiffLineOp::Equal(b"    UnpackFile {\n"),
        ];

        normalize_diff_change_run_structural_common_prefix_to_equal_in_place(&mut ops);

        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (b' ', b"    Help {\n".to_vec()),
                (
                    b' ',
                    b"        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]\n"
                        .to_vec(),
                ),
                (b' ', b"        args: Vec<String>,\n".to_vec()),
                (b' ', b"    },\n".to_vec()),
                (b'+', b"    Lfs {\n".to_vec()),
                (
                    b'+',
                    b"        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]\n"
                        .to_vec(),
                ),
                (b'+', b"        args: Vec<String>,\n".to_vec()),
                (b'+', b"    },\n".to_vec()),
                (b' ', b"    UnpackFile {\n".to_vec()),
            ]
        );
    }

    #[test]
    fn unified_hunk_matches_stock_shape_for_enum_insert_on_real_schema_slice() {
        let old = br#"    Help {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    UnpackFile {
        object: String,
    },
"#;
        let new = br#"    Help {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Lfs {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    UnpackFile {
        object: String,
    },
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "crates/zmin-cli-schema/src/lib.rs",
            HunkFormatOptions::default(),
        )
        .expect("write enum insert hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            concat!(
                "@@ -2,6 +2,10 @@\n",
                "         #[arg(trailing_var_arg = true, allow_hyphen_values = true)]\n",
                "         args: Vec<String>,\n",
                "     },\n",
                "+    Lfs {\n",
                "+        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]\n",
                "+        args: Vec<String>,\n",
                "+    },\n",
                "     UnpackFile {\n",
                "         object: String,\n",
                "     },\n",
            )
        );
    }

    #[test]
    fn unified_hunk_keeps_inserted_duplicate_cfg_before_existing_cfg_context() {
        let old = br#"    }
}

#[cfg(unix)]
fn credential_cache_send_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
) -> Result<()> {
    Ok(())
}
"#;
        let new = br#"    }
}

#[cfg(unix)]
fn credential_cache_request(
    socket: &std::path::Path,
    timeout: Option<u64>,
    action: &str,
) -> Result<String> {
    Ok(String::new())
}

#[cfg(unix)]
fn credential_cache_send_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
) -> Result<()> {
    Ok(())
}
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "credential_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write credential cfg hunk");

        let rendered = String::from_utf8(out).expect("utf8");
        assert!(rendered.starts_with("@@ -"));
        assert!(rendered.contains(" }\n \n+#[cfg(unix)]\n+fn credential_cache_request(\n"));
        assert!(rendered.contains("+}\n+\n #[cfg(unix)]\n fn credential_cache_send_to_stream(\n"));
    }

    #[test]
    fn diff_ops_keep_blank_context_before_inserted_duplicate_cfg() {
        let old = br#"    }
}

#[cfg(unix)]
fn credential_cache_send_to_stream(
"#;
        let new = br#"    }
}

#[cfg(unix)]
fn credential_cache_request(
    socket: &std::path::Path,
) -> Result<String> {
    Ok(String::new())
}

#[cfg(unix)]
fn credential_cache_send_to_stream(
"#;
        let ops = diff_line_ops(&split_diff_lines(old), &split_diff_lines(new));
        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (b' ', b"    }\n".to_vec()),
                (b' ', b"}\n".to_vec()),
                (b' ', b"\n".to_vec()),
                (b'+', b"#[cfg(unix)]\n".to_vec()),
                (b'+', b"fn credential_cache_request(\n".to_vec()),
                (b'+', b"    socket: &std::path::Path,\n".to_vec()),
                (b'+', b") -> Result<String> {\n".to_vec()),
                (b'+', b"    Ok(String::new())\n".to_vec()),
                (b'+', b"}\n".to_vec()),
                (b'+', b"\n".to_vec()),
                (b' ', b"#[cfg(unix)]\n".to_vec()),
                (b' ', b"fn credential_cache_send_to_stream(\n".to_vec()),
            ]
        );
    }

    #[test]
    fn diff_ops_keep_blank_context_for_real_credential_request_insert() {
        let old = br#"    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(mut stream) => credential_cache_send_to_stream(&mut stream, action, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_send_to_stream(&mut stream, action, entries)
        }
    }
}

#[cfg(unix)]
fn credential_cache_send_to_stream(
"#;
        let new = br#"    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(mut stream) => credential_cache_send_to_stream(&mut stream, action, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_send_to_stream(&mut stream, action, entries)
        }
    }
}

#[cfg(unix)]
fn credential_cache_request(
    socket: &std::path::Path,
    timeout: Option<u64>,
    action: &str,
    entries: &[(String, String)],
) -> Result<String> {
    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(mut stream) => credential_cache_request_to_stream(&mut stream, action, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_request_to_stream(&mut stream, action, entries)
        }
    }
}

#[cfg(unix)]
fn credential_cache_send_to_stream(
"#;
        let ops = diff_line_ops(&split_diff_lines(old), &split_diff_lines(new));
        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (
                    b' ',
                    b"    match std::os::unix::net::UnixStream::connect(socket) {\n".to_vec(),
                ),
                (
                    b' ',
                    b"        Ok(mut stream) => credential_cache_send_to_stream(&mut stream, action, entries),\n"
                        .to_vec(),
                ),
                (b' ', b"        Err(_) => {\n".to_vec()),
                (
                    b' ',
                    b"            start_credential_cache_daemon(socket, timeout)?;\n".to_vec(),
                ),
                (
                    b' ',
                    b"            let mut stream = connect_credential_cache_daemon(socket)?;\n".to_vec(),
                ),
                (
                    b' ',
                    b"            credential_cache_send_to_stream(&mut stream, action, entries)\n"
                        .to_vec(),
                ),
                (b' ', b"        }\n".to_vec()),
                (b' ', b"    }\n".to_vec()),
                (b' ', b"}\n".to_vec()),
                (b' ', b"\n".to_vec()),
                (b'+', b"#[cfg(unix)]\n".to_vec()),
                (b'+', b"fn credential_cache_request(\n".to_vec()),
                (b'+', b"    socket: &std::path::Path,\n".to_vec()),
                (b'+', b"    timeout: Option<u64>,\n".to_vec()),
                (b'+', b"    action: &str,\n".to_vec()),
                (b'+', b"    entries: &[(String, String)],\n".to_vec()),
                (b'+', b") -> Result<String> {\n".to_vec()),
                (
                    b'+',
                    b"    match std::os::unix::net::UnixStream::connect(socket) {\n".to_vec(),
                ),
                (
                    b'+',
                    b"        Ok(mut stream) => credential_cache_request_to_stream(&mut stream, action, entries),\n"
                        .to_vec(),
                ),
                (b'+', b"        Err(_) => {\n".to_vec()),
                (
                    b'+',
                    b"            start_credential_cache_daemon(socket, timeout)?;\n".to_vec(),
                ),
                (
                    b'+',
                    b"            let mut stream = connect_credential_cache_daemon(socket)?;\n".to_vec(),
                ),
                (
                    b'+',
                    b"            credential_cache_request_to_stream(&mut stream, action, entries)\n"
                        .to_vec(),
                ),
                (b'+', b"        }\n".to_vec()),
                (b'+', b"    }\n".to_vec()),
                (b'+', b"}\n".to_vec()),
                (b'+', b"\n".to_vec()),
                (b' ', b"#[cfg(unix)]\n".to_vec()),
                (b' ', b"fn credential_cache_send_to_stream(\n".to_vec()),
            ]
        );
    }

    #[test]
    fn diff_ops_myers_keep_blank_context_for_real_credential_request_insert() {
        let old = br#"    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(mut stream) => credential_cache_send_to_stream(&mut stream, action, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_send_to_stream(&mut stream, action, entries)
        }
    }
}

#[cfg(unix)]
fn credential_cache_send_to_stream(
"#;
        let new = br#"    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(mut stream) => credential_cache_send_to_stream(&mut stream, action, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_send_to_stream(&mut stream, action, entries)
        }
    }
}

#[cfg(unix)]
fn credential_cache_request(
    socket: &std::path::Path,
    timeout: Option<u64>,
    action: &str,
    entries: &[(String, String)],
) -> Result<String> {
    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(mut stream) => credential_cache_request_to_stream(&mut stream, action, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_request_to_stream(&mut stream, action, entries)
        }
    }
}

#[cfg(unix)]
fn credential_cache_send_to_stream(
"#;
        let ops = diff_line_ops_myers(&split_diff_lines(old), &split_diff_lines(new));
        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (
                    b' ',
                    b"    match std::os::unix::net::UnixStream::connect(socket) {\n".to_vec(),
                ),
                (
                    b' ',
                    b"        Ok(mut stream) => credential_cache_send_to_stream(&mut stream, action, entries),\n"
                        .to_vec(),
                ),
                (b' ', b"        Err(_) => {\n".to_vec()),
                (
                    b' ',
                    b"            start_credential_cache_daemon(socket, timeout)?;\n".to_vec(),
                ),
                (
                    b' ',
                    b"            let mut stream = connect_credential_cache_daemon(socket)?;\n".to_vec(),
                ),
                (
                    b' ',
                    b"            credential_cache_send_to_stream(&mut stream, action, entries)\n"
                        .to_vec(),
                ),
                (b' ', b"        }\n".to_vec()),
                (b' ', b"    }\n".to_vec()),
                (b' ', b"}\n".to_vec()),
                (b' ', b"\n".to_vec()),
                (b' ', b"#[cfg(unix)]\n".to_vec()),
                (b'+', b"fn credential_cache_request(\n".to_vec()),
                (b'+', b"    socket: &std::path::Path,\n".to_vec()),
                (b'+', b"    timeout: Option<u64>,\n".to_vec()),
                (b'+', b"    action: &str,\n".to_vec()),
                (b'+', b"    entries: &[(String, String)],\n".to_vec()),
                (b'+', b") -> Result<String> {\n".to_vec()),
                (
                    b'+',
                    b"    match std::os::unix::net::UnixStream::connect(socket) {\n".to_vec(),
                ),
                (
                    b'+',
                    b"        Ok(mut stream) => credential_cache_request_to_stream(&mut stream, action, entries),\n"
                        .to_vec(),
                ),
                (b'+', b"        Err(_) => {\n".to_vec()),
                (
                    b'+',
                    b"            start_credential_cache_daemon(socket, timeout)?;\n".to_vec(),
                ),
                (
                    b'+',
                    b"            let mut stream = connect_credential_cache_daemon(socket)?;\n".to_vec(),
                ),
                (
                    b'+',
                    b"            credential_cache_request_to_stream(&mut stream, action, entries)\n"
                        .to_vec(),
                ),
                (b'+', b"        }\n".to_vec()),
                (b'+', b"    }\n".to_vec()),
                (b'+', b"}\n".to_vec()),
                (b'+', b"\n".to_vec()),
                (b'+', b"#[cfg(unix)]\n".to_vec()),
                (b' ', b"fn credential_cache_send_to_stream(\n".to_vec()),
            ]
        );
    }

    #[test]
    fn unified_hunk_matches_stock_shape_for_real_credential_cache_request_slice() {
        let old = br#"        return Ok(PathBuf::from(cache_home).join("git/credential/socket"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "credential-cache requires HOME, XDG_CACHE_HOME, or --socket".into(),
    })?;
    Ok(PathBuf::from(home).join(".cache/git/credential/socket"))
}

#[cfg(unix)]
fn credential_cache_send(
    socket: &std::path::Path,
    timeout: Option<u64>,
    action: &str,
    entries: &[(String, String)],
) -> Result<()> {
    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(mut stream) => credential_cache_send_to_stream(&mut stream, action, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_send_to_stream(&mut stream, action, entries)
        }
    }
}

#[cfg(unix)]
fn credential_cache_send_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<()> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{response}");
    Ok(())
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
    if let Some(parent) = socket.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    if socket.exists() {
        let _ = fs::remove_file(socket);
    }
    let mut command = ProcessCommand::new(std::env::current_exe()?);
    command
        .arg("credential-cache")
        .arg("--daemon-internal")
        .arg("--socket")
        .arg(socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
"#;
        let new = br#"    if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(cache_home).join("git/credential/socket"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "credential-cache requires HOME, XDG_CACHE_HOME, or --socket".into(),
    })?;
    Ok(PathBuf::from(home).join(".cache/git/credential/socket"))
}

#[cfg(unix)]
fn credential_cache_send(
    socket: &std::path::Path,
    timeout: Option<u64>,
    action: &str,
    entries: &[(String, String)],
) -> Result<()> {
    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(mut stream) => credential_cache_send_to_stream(&mut stream, action, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_send_to_stream(&mut stream, action, entries)
        }
    }
}

#[cfg(unix)]
fn credential_cache_request(
    socket: &std::path::Path,
    timeout: Option<u64>,
    action: &str,
    entries: &[(String, String)],
) -> Result<String> {
    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(mut stream) => credential_cache_request_to_stream(&mut stream, action, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_request_to_stream(&mut stream, action, entries)
        }
    }
}

#[cfg(unix)]
fn credential_cache_send_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<()> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{response}");
    Ok(())
}

#[cfg(unix)]
fn credential_cache_request_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<String> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "credential_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write credential cache request slice hunk");

        let rendered = String::from_utf8(out).expect("utf8");
        assert!(
            rendered.contains("@@ -24,6 +25,23 @@ fn credential_cache_send("),
            "{}",
            rendered
        );
        assert!(
            rendered.contains(" }\n \n+#[cfg(unix)]\n+fn credential_cache_request(\n"),
            "{}",
            rendered
        );
        assert!(
            rendered.contains(
                "+        }\n+    }\n+}\n+\n #[cfg(unix)]\n fn credential_cache_send_to_stream(\n"
            ),
            "{}",
            rendered
        );
    }

    #[test]
    #[ignore = "known parity gap: inserted helper epilogue before following cfg declaration"]
    fn unified_hunk_keeps_inserted_credential_helper_epilogue_before_following_cfg() {
        let old = br#"#[cfg(unix)]
fn credential_cache_send_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<()> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{response}");
    Ok(())
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
"#;
        let new = br#"#[cfg(unix)]
fn credential_cache_request_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<String> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
"#;

        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "credential_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write credential helper epilogue hunk");

        let rendered = String::from_utf8(out).expect("utf8");
        assert!(
            rendered.contains("+    stream.read_to_string(&mut response)?;\n+    Ok(response)\n+}\n+\n #[cfg(unix)]\n fn start_credential_cache_daemon("),
            "{}",
            rendered
        );
    }

    #[test]
    #[ignore = "debug exact observed credential fragment from HEAD~1..HEAD"]
    fn debug_exact_observed_credential_fragment_hunk() {
        let old = br#"    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{response}");
    Ok(())
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
    if let Some(parent) = socket.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    if socket.exists() {
        let _ = fs::remove_file(socket);
    }
    let mut command = ProcessCommand::new(std::env::current_exe()?);
    command
        .arg("credential-cache")
        .arg("--daemon-internal")
        .arg("--socket")
        .arg(socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(timeout) = timeout {
        command.arg("--timeout").arg(timeout.to_string());
    }
    command.spawn().map_err(CliError::Io)?;
    Ok(())
"#;
        let new = br#"    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{response}");
    Ok(())
}

#[cfg(unix)]
fn credential_cache_request_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<String> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
    if let Some(parent) = socket.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    if socket.exists() {
        let _ = fs::remove_file(socket);
    }
    let mut command = ProcessCommand::new(std::env::current_exe()?);
    command
        .arg("credential-cache")
        .arg("--daemon-internal")
        .arg("--socket")
        .arg(socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(timeout) = timeout {
        command.arg("--timeout").arg(timeout.to_string());
    }
    command.spawn().map_err(CliError::Io)?;
    Ok(())
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "credential_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write exact credential fragment hunk");
        panic!("{}", String::from_utf8(out).expect("utf8"));
    }

    #[test]
    #[ignore = "debug exact observed credential fragment ops from HEAD~1..HEAD"]
    fn debug_exact_observed_credential_fragment_ops() {
        let old = br#"    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{response}");
    Ok(())
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
    if let Some(parent) = socket.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    if socket.exists() {
        let _ = fs::remove_file(socket);
    }
    let mut command = ProcessCommand::new(std::env::current_exe()?);
    command
        .arg("credential-cache")
        .arg("--daemon-internal")
        .arg("--socket")
        .arg(socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(timeout) = timeout {
        command.arg("--timeout").arg(timeout.to_string());
    }
    command.spawn().map_err(CliError::Io)?;
    Ok(())
"#;
        let new = br#"    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{response}");
    Ok(())
}

#[cfg(unix)]
fn credential_cache_request_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<String> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
    if let Some(parent) = socket.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    if socket.exists() {
        let _ = fs::remove_file(socket);
    }
    let mut command = ProcessCommand::new(std::env::current_exe()?);
    command
        .arg("credential-cache")
        .arg("--daemon-internal")
        .arg("--socket")
        .arg(socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(timeout) = timeout {
        command.arg("--timeout").arg(timeout.to_string());
    }
    command.spawn().map_err(CliError::Io)?;
    Ok(())
"#;
        panic!("{:?}", debug_line_tags(old, new));
    }

    #[test]
    #[ignore = "known parity gap: extra inserted top-level test attribute before following test"]
    fn unified_hunk_does_not_insert_extra_blank_before_next_top_level_test() {
        let old = br#"#[test]
fn managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation() {
    let repo = git_init();

    assert_eq!(run_zmin(repo.path(), ["hooks", "list"]), "");
    assert!(!repo.path().join(".git/hooks/pre-receive").exists());
}

#[test]
fn version_command_reports_git_compatible_version_shape() {
    let repo = git_init();
}
"#;
        let new = br#"#[test]
fn managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation() {
    let repo = git_init();

    assert_eq!(run_zmin(repo.path(), ["hooks", "list"]), "");
    assert!(!repo.path().join(".git/hooks/pre-receive").exists());
}

#[test]
fn managed_hooks_run_staged_list_uses_index_backed_selector() {
    let repo = git_init();
    configure_identity(repo.path());
}

#[test]
fn version_command_reports_git_compatible_version_shape() {
    let repo = git_init();
}
"#;

        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "git_admin_tools_compat.rs",
            HunkFormatOptions::default(),
        )
        .expect("write top-level test insertion hunk");

        let rendered = String::from_utf8(out).expect("utf8");
        assert!(
            rendered.contains("+    configure_identity(repo.path());\n+}\n+\n #[test]\n fn version_command_reports_git_compatible_version_shape() {\n"),
            "{}",
            rendered
        );
        assert!(!rendered.contains("+}\n+\n+#[test]\n"), "{}", rendered);
    }

    #[test]
    #[ignore = "known parity gap on Myers path for top-level test insertion"]
    fn diff_ops_myers_does_not_insert_extra_top_level_test_before_following_test() {
        let old = br#"#[test]
fn managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation() {
    let repo = git_init();

    assert_eq!(run_zmin(repo.path(), ["hooks", "list"]), "");
    assert!(!repo.path().join(".git/hooks/pre-receive").exists());
}

#[test]
fn version_command_reports_git_compatible_version_shape() {
    let repo = git_init();
}
"#;
        let new = br#"#[test]
fn managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation() {
    let repo = git_init();

    assert_eq!(run_zmin(repo.path(), ["hooks", "list"]), "");
    assert!(!repo.path().join(".git/hooks/pre-receive").exists());
}

#[test]
fn managed_hooks_run_staged_list_uses_index_backed_selector() {
    let repo = git_init();
    configure_identity(repo.path());
}

#[test]
fn version_command_reports_git_compatible_version_shape() {
    let repo = git_init();
}
"#;
        let ops = diff_line_ops_myers(&split_diff_lines(old), &split_diff_lines(new));
        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (b' ', b"#[test]\n".to_vec()),
                (
                    b' ',
                    b"fn managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation() {\n"
                        .to_vec(),
                ),
                (b' ', b"    let repo = git_init();\n".to_vec()),
                (b' ', b"\n".to_vec()),
                (
                    b' ',
                    b"    assert_eq!(run_zmin(repo.path(), [\"hooks\", \"list\"]), \"\");\n"
                        .to_vec(),
                ),
                (
                    b' ',
                    b"    assert!(!repo.path().join(\".git/hooks/pre-receive\").exists());\n"
                        .to_vec(),
                ),
                (b' ', b"}\n".to_vec()),
                (b' ', b"\n".to_vec()),
                (b'+', b"#[test]\n".to_vec()),
                (
                    b'+',
                    b"fn managed_hooks_run_staged_list_uses_index_backed_selector() {\n".to_vec(),
                ),
                (b'+', b"    let repo = git_init();\n".to_vec()),
                (b'+', b"    configure_identity(repo.path());\n".to_vec()),
                (b'+', b"}\n".to_vec()),
                (b'+', b"\n".to_vec()),
                (b' ', b"#[test]\n".to_vec()),
                (
                    b' ',
                    b"fn version_command_reports_git_compatible_version_shape() {\n".to_vec(),
                ),
                (b' ', b"    let repo = git_init();\n".to_vec()),
                (b' ', b"}\n".to_vec()),
            ]
        );
    }

    #[test]
    fn unified_hunk_keeps_inserted_top_level_test_attribute_with_insert_block() {
        let old = br#"#[test]
fn managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation() {
    let repo = git_init();

    assert_eq!(run_zmin(repo.path(), ["hooks", "list"]), "");
    assert!(!repo.path().join(".git/hooks/pre-receive").exists());
}

#[test]
fn version_command_reports_git_compatible_version_shape() {
    let repo = git_init();
}
"#;
        let new = br#"#[test]
fn managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation() {
    let repo = git_init();

    assert_eq!(run_zmin(repo.path(), ["hooks", "list"]), "");
    assert!(!repo.path().join(".git/hooks/pre-receive").exists());
}

#[test]
fn managed_hooks_run_staged_list_uses_index_backed_selector() {
    let repo = git_init();
    configure_identity(repo.path());
}

#[test]
fn version_command_reports_git_compatible_version_shape() {
    let repo = git_init();
}
"#;

        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "git_admin_tools_compat.rs",
            HunkFormatOptions::default(),
        )
        .expect("write top-level test insertion hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            concat!(
                "@@ -6,6 +6,12 @@ fn managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation() {\n",
                "     assert!(!repo.path().join(\".git/hooks/pre-receive\").exists());\n",
                " }\n",
                " \n",
                "+#[test]\n",
                "+fn managed_hooks_run_staged_list_uses_index_backed_selector() {\n",
                "+    let repo = git_init();\n",
                "+    configure_identity(repo.path());\n",
                "+}\n",
                "+\n",
                " #[test]\n",
                " fn version_command_reports_git_compatible_version_shape() {\n",
                "     let repo = git_init();\n",
            )
        );
    }

    #[test]
    fn unified_hunk_keeps_inserted_top_level_test_attribute_with_longer_realistic_body() {
        let old = br#"            String::new(),
            "fatal: unsupported hook 'pre-receive'".to_owned()
        )
    );
    assert_eq!(run_zmin(repo.path(), ["hooks", "list"]), "");
    assert!(!repo.path().join(".git/hooks/pre-receive").exists());
}

#[test]
fn version_command_reports_git_compatible_version_shape() {
    let repo = git_init();

    let version = command_any_with_git_editor(zmin_bin(), repo.path(), &["version"]);
    assert_eq!(version.0, 0);
    assert!(version.1.starts_with("git version "));
    assert!(version.1.contains("(zmin "));
    assert_eq!(version.2, "");
}
"#;
        let new = br#"            String::new(),
            "fatal: unsupported hook 'pre-receive'".to_owned()
        )
    );
    assert_eq!(run_zmin(repo.path(), ["hooks", "list"]), "");
    assert!(!repo.path().join(".git/hooks/pre-receive").exists());
}

#[test]
fn managed_hooks_run_staged_list_uses_index_backed_selector() {
    let repo = git_init();
    configure_identity(repo.path());

    assert_eq!(
        run_zmin(
            repo.path(),
            ["hooks", "run", "pre-commit", "--staged", "--list"]
        ),
        ""
    );

    write_file(repo.path(), "tracked.txt", "base\n");
    write_file(repo.path(), "rename-old.txt", "rename\n");
    write_file(repo.path(), "delete.txt", "delete\n");
}

#[test]
fn version_command_reports_git_compatible_version_shape() {
    let repo = git_init();

    let version = command_any_with_git_editor(zmin_bin(), repo.path(), &["version"]);
    assert_eq!(version.0, 0);
    assert!(version.1.starts_with("git version "));
    assert!(version.1.contains("(zmin "));
    assert_eq!(version.2, "");
}
"#;

        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "git_admin_tools_compat.rs",
            HunkFormatOptions::default(),
        )
        .expect("write longer top-level test insertion hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            concat!(
                "@@ -6,6 +6,24 @@\n",
                "     assert!(!repo.path().join(\".git/hooks/pre-receive\").exists());\n",
                " }\n",
                " \n",
                "+#[test]\n",
                "+fn managed_hooks_run_staged_list_uses_index_backed_selector() {\n",
                "+    let repo = git_init();\n",
                "+    configure_identity(repo.path());\n",
                "+\n",
                "+    assert_eq!(\n",
                "+        run_zmin(\n",
                "+            repo.path(),\n",
                "+            [\"hooks\", \"run\", \"pre-commit\", \"--staged\", \"--list\"]\n",
                "+        ),\n",
                "+        \"\"\n",
                "+    );\n",
                "+\n",
                "+    write_file(repo.path(), \"tracked.txt\", \"base\\n\");\n",
                "+    write_file(repo.path(), \"rename-old.txt\", \"rename\\n\");\n",
                "+    write_file(repo.path(), \"delete.txt\", \"delete\\n\");\n",
                "+}\n",
                "+\n",
                " #[test]\n",
                " fn version_command_reports_git_compatible_version_shape() {\n",
                "     let repo = git_init();\n",
            )
        );
    }

    #[test]
    fn unified_hunk_keeps_markdown_heading_as_context_after_no_staged_behavior_split() {
        let old = br#"Filtering order:

1. collect staged entries from index versus `HEAD`
2. normalize paths to repository-root relative slash paths
3. apply optional pathspec filters
4. apply optional extension filters
5. drop non-executable deleted entries from command arguments
6. preserve stable index order for deterministic output

No-staged-file behavior:

- `--list` exits `0` and prints no selected executable paths
- command mode exits `0` without running the command
- `--dry-run` exits `0` and prints that the command would not run

## Output Contract

`--list` output is line oriented and stable for tests:
"#;
        let new = br#"Filtering order:

1. collect staged entries from index versus `HEAD`
2. normalize paths to repository-root relative slash paths
3. apply optional extension filters
4. apply optional pathspec filters
5. drop non-executable deleted entries from command arguments
6. preserve stable index order for deterministic output

Current no-staged-file behavior:

- `--list` exits `0` and prints no selected executable paths

Planned no-staged-file behavior after command mode lands:

- pathspec-filtered command mode exits `0` without running the command
- pathspec-filtered `--dry-run` exits `0` and prints that the command would not run

## Output Contract

`--list` output is line oriented and stable for tests:
"#;

        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "zmin_hooks_staged_runner.md",
            HunkFormatOptions::default(),
        )
        .expect("write markdown heading split hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            concat!(
                "@@ -2,16 +2,19 @@ Filtering order:\n",
                " \n",
                " 1. collect staged entries from index versus `HEAD`\n",
                " 2. normalize paths to repository-root relative slash paths\n",
                "-3. apply optional pathspec filters\n",
                "-4. apply optional extension filters\n",
                "+3. apply optional extension filters\n",
                "+4. apply optional pathspec filters\n",
                " 5. drop non-executable deleted entries from command arguments\n",
                " 6. preserve stable index order for deterministic output\n",
                " \n",
                "-No-staged-file behavior:\n",
                "+Current no-staged-file behavior:\n",
                " \n",
                " - `--list` exits `0` and prints no selected executable paths\n",
                "-- command mode exits `0` without running the command\n",
                "-- `--dry-run` exits `0` and prints that the command would not run\n",
                "+\n",
                "+Planned no-staged-file behavior after command mode lands:\n",
                "+\n",
                "+- pathspec-filtered command mode exits `0` without running the command\n",
                "+- pathspec-filtered `--dry-run` exits `0` and prints that the command would not run\n",
                " \n",
                " ## Output Contract\n",
                " \n",
            )
        );
    }

    #[test]
    fn unified_hunk_keeps_markdown_output_contract_line_in_following_hunk() {
        let old = br#"The runner returns the child exit code. Spawn failures return Zmin validation
errors with a non-zero exit code and no stock-Git compatibility claim.

## Managed Hook Integration

Managed hooks remain optional. A generated `.git/hooks/pre-commit` wrapper may
call the staged runner, but manual hooks must remain untouched unless the user
explicitly opts into `zmin hooks add --force`.

The wrapper should be small and inspectable:

```sh
#!/bin/sh
zmin hooks run pre-commit --staged -- "$@"
```

Project-specific commands should live in Git config or an explicit checked-in
script before automatic wrapper generation is expanded. Avoid hidden defaults.

## Evidence Plan
"#;
        let new = br#"The runner returns the child exit code. Spawn failures return Zmin validation
errors with a non-zero exit code and no stock-Git compatibility claim.

Current `--dry-run` output prints the exact command preview with selected
paths shell-quoted on one line. When no executable staged paths remain after
filtering it prints:

```text
would not run: no staged executable paths selected
```

## Managed Hook Integration

Managed hooks remain optional. A generated `.git/hooks/pre-commit` wrapper may
call the staged runner, but manual hooks must remain untouched unless the user
explicitly opts into `zmin hooks add --force`.

The wrapper is intentionally small and inspectable:

```sh
#!/bin/sh
exec /path/to/zmin hooks run pre-commit --staged "$@"
```

Project-specific command and extension selection live in Git config via
`zmin hooks add --staged-runner ... -- <command> [args...]`. The wrapper only
delegates to `zmin hooks run pre-commit --staged`, so the tracked behavior is:

- selected extensions and command words are stored in
  `zmin.hooks-runner.<hook>`
- commit-time `pre-commit` execution reuses the same staged selector contract
- `zmin hooks remove <hook>` clears both managed-hook config modes and removes
  the owned wrapper file

## Evidence Plan
"#;

        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "zmin_hooks_staged_runner.md",
            HunkFormatOptions::default(),
        )
        .expect("write markdown output contract split hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            concat!(
                "@@ -1,20 +1,35 @@\n",
                " The runner returns the child exit code. Spawn failures return Zmin validation\n",
                " errors with a non-zero exit code and no stock-Git compatibility claim.\n",
                " \n",
                "+Current `--dry-run` output prints the exact command preview with selected\n",
                "+paths shell-quoted on one line. When no executable staged paths remain after\n",
                "+filtering it prints:\n",
                "+\n",
                "+```text\n",
                "+would not run: no staged executable paths selected\n",
                "+```\n",
                "+\n",
                " ## Managed Hook Integration\n",
                " \n",
                " Managed hooks remain optional. A generated `.git/hooks/pre-commit` wrapper may\n",
                " call the staged runner, but manual hooks must remain untouched unless the user\n",
                " explicitly opts into `zmin hooks add --force`.\n",
                " \n",
                "-The wrapper should be small and inspectable:\n",
                "+The wrapper is intentionally small and inspectable:\n",
                " \n",
                " ```sh\n",
                " #!/bin/sh\n",
                "-zmin hooks run pre-commit --staged -- \"$@\"\n",
                "+exec /path/to/zmin hooks run pre-commit --staged \"$@\"\n",
                " ```\n",
                " \n",
                "-Project-specific commands should live in Git config or an explicit checked-in\n",
                "-script before automatic wrapper generation is expanded. Avoid hidden defaults.\n",
                "+Project-specific command and extension selection live in Git config via\n",
                "+`zmin hooks add --staged-runner ... -- <command> [args...]`. The wrapper only\n",
                "+delegates to `zmin hooks run pre-commit --staged`, so the tracked behavior is:\n",
                "+\n",
                "+- selected extensions and command words are stored in\n",
                "+  `zmin.hooks-runner.<hook>`\n",
                "+- commit-time `pre-commit` execution reuses the same staged selector contract\n",
                "+- `zmin hooks remove <hook>` clears both managed-hook config modes and removes\n",
                "+  the owned wrapper file\n",
                " \n",
                " ## Evidence Plan\n",
            )
        );
    }

    #[test]
    #[ignore = "diagnostic for realistic top-level inserted test parity gap"]
    fn debug_realistic_top_level_inserted_test_ops() {
        let old = br#"            String::new(),
            "fatal: unsupported hook 'pre-receive'".to_owned()
        )
    );
    assert_eq!(run_zmin(repo.path(), ["hooks", "list"]), "");
    assert!(!repo.path().join(".git/hooks/pre-receive").exists());
}

#[test]
fn version_command_reports_git_compatible_version_shape() {
    let repo = git_init();

    let version = command_any_with_git_editor(zmin_bin(), repo.path(), &["version"]);
    assert_eq!(version.0, 0);
    assert!(version.1.starts_with("git version "));
    assert!(version.1.contains("(zmin "));
    assert_eq!(version.2, "");
}
"#;
        let new = br#"            String::new(),
            "fatal: unsupported hook 'pre-receive'".to_owned()
        )
    );
    assert_eq!(run_zmin(repo.path(), ["hooks", "list"]), "");
    assert!(!repo.path().join(".git/hooks/pre-receive").exists());
}

#[test]
fn managed_hooks_run_staged_list_uses_index_backed_selector() {
    let repo = git_init();
    configure_identity(repo.path());

    assert_eq!(
        run_zmin(
            repo.path(),
            ["hooks", "run", "pre-commit", "--staged", "--list"]
        ),
        ""
    );

    write_file(repo.path(), "tracked.txt", "base\n");
    write_file(repo.path(), "rename-old.txt", "rename\n");
    write_file(repo.path(), "delete.txt", "delete\n");
}

#[test]
fn version_command_reports_git_compatible_version_shape() {
    let repo = git_init();

    let version = command_any_with_git_editor(zmin_bin(), repo.path(), &["version"]);
    assert_eq!(version.0, 0);
    assert!(version.1.starts_with("git version "));
    assert!(version.1.contains("(zmin "));
    assert_eq!(version.2, "");
}
"#;
        let ops = diff_line_ops_with_whitespace(
            &split_diff_lines(old),
            &split_diff_lines(new),
            DiffWhitespaceMode::None,
        );
        panic!("{:?}", diff_line_op_tags(&ops));
    }

    #[test]
    fn diff_ops_myers_keeps_inserted_helper_epilogue_before_following_cfg() {
        let old = br#"#[cfg(unix)]
fn credential_cache_send_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<()> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{response}");
    Ok(())
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
"#;
        let new = br#"#[cfg(unix)]
fn credential_cache_request_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<String> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
"#;
        let ops = diff_line_ops_myers(&split_diff_lines(old), &split_diff_lines(new));
        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (b' ', b"#[cfg(unix)]\n".to_vec()),
                (b'-', b"fn credential_cache_send_to_stream(\n".to_vec()),
                (b'+', b"fn credential_cache_request_to_stream(\n".to_vec()),
                (
                    b' ',
                    b"    stream: &mut std::os::unix::net::UnixStream,\n".to_vec(),
                ),
                (b' ', b"    action: &str,\n".to_vec()),
                (b' ', b"    entries: &[(String, String)],\n".to_vec()),
                (b'-', b") -> Result<()> {\n".to_vec()),
                (b'+', b") -> Result<String> {\n".to_vec()),
                (b' ', b"    let mut request = String::new();\n".to_vec()),
                (b' ', b"    request.push_str(action);\n".to_vec()),
                (b' ', b"    request.push('\\n');\n".to_vec()),
                (b' ', b"    for (key, value) in entries {\n".to_vec()),
                (b' ', b"        request.push_str(key);\n".to_vec()),
                (b' ', b"        request.push('=');\n".to_vec()),
                (b' ', b"        request.push_str(value);\n".to_vec()),
                (b' ', b"        request.push('\\n');\n".to_vec()),
                (b' ', b"    }\n".to_vec()),
                (b' ', b"    request.push('\\n');\n".to_vec()),
                (b' ', b"    stream.write_all(request.as_bytes())?;\n".to_vec()),
                (b' ', b"    stream\n".to_vec()),
                (
                    b' ',
                    b"        .shutdown(std::net::Shutdown::Write)\n".to_vec(),
                ),
                (b' ', b"        .map_err(CliError::Io)?;\n".to_vec()),
                (b' ', b"    let mut response = String::new();\n".to_vec()),
                (b' ', b"    stream.read_to_string(&mut response)?;\n".to_vec()),
                (b'-', b"    print!(\"{response}\");\n".to_vec()),
                (b'-', b"    Ok(())\n".to_vec()),
                (b'+', b"    Ok(response)\n".to_vec()),
                (b' ', b"}\n".to_vec()),
                (b' ', b"\n".to_vec()),
                (b' ', b"#[cfg(unix)]\n".to_vec()),
                (
                    b' ',
                    b"fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {\n"
                        .to_vec(),
                ),
            ]
        );
    }

    #[test]
    #[ignore = "debug render_ops for credential helper epilogue parity"]
    fn debug_render_ops_credential_helper_epilogue() {
        let old = br#"#[cfg(unix)]
fn credential_cache_send_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<()> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{response}");
    Ok(())
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
"#;
        let new = br#"#[cfg(unix)]
fn credential_cache_request_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<String> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
"#;
        panic!("{:?}", rendered_hunk_op_tags(old, new));
    }

    #[test]
    #[ignore = "debug lcs ops for credential helper epilogue parity"]
    fn debug_lcs_ops_credential_helper_epilogue() {
        let old = br#"#[cfg(unix)]
fn credential_cache_send_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<()> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{response}");
    Ok(())
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
"#;
        let new = br#"#[cfg(unix)]
fn credential_cache_request_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<String> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
"#;
        let ops = diff_line_ops_with_whitespace(
            &split_diff_lines(old),
            &split_diff_lines(new),
            DiffWhitespaceMode::None,
        );
        panic!("{:?}", diff_line_op_tags(&ops));
    }

    #[test]
    #[ignore = "known parity gap on Myers path for inserted transport test before following test"]
    fn diff_ops_myers_does_not_collapse_inserted_transport_test_into_following_test() {
        let old = br#"    assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
}

#[test]
fn parsed_http_url_keeps_url_userinfo_before_credential_store() {
    let dir = tempfile::TempDir::new().expect("repo");
    let repo = test_repo_at(dir.path());
    let credentials = dir.path().join("credentials");
    std::fs::write(&credentials, "https://stored:secret@example.test\n").expect("credentials");
    std::fs::write(
        repo.git_dir.join("config"),
        format!(
            "[credential]\n\thelper = store --file {}\n",
            credentials.display()
        ),
    )
    .expect("config");

    let url = parsed_http_url_with_extra_headers(
        Some(&repo),
        "https://url-user:url-pass@example.test/repo.git",
    )
    .expect("parsed URL with credential helper");
"#;
        let new = br#"    assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
}

#[test]
fn parsed_http_url_reads_credential_store_helper_basic_auth_with_quoted_file_path() {
    let dir = tempfile::TempDir::new().expect("repo");
    let repo = test_repo_at(dir.path());
    let credentials_dir = dir.path().join("folder with spaces");
    std::fs::create_dir_all(&credentials_dir).expect("credentials dir");
    let credentials = credentials_dir.join("quoted credentials");
    std::fs::write(&credentials, "https://user:p%40ss@example.test\n").expect("credentials");
    std::fs::write(
        repo.git_dir.join("config"),
        format!(
            "[credential]\n\thelper = store --file '{}'\n",
            credentials.display()
        ),
    )
    .expect("config");

    let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
        .expect("parsed URL with credential helper");

    assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
}

#[test]
fn parsed_http_url_keeps_url_userinfo_before_credential_store() {
    let dir = tempfile::TempDir::new().expect("repo");
    let repo = test_repo_at(dir.path());
    let credentials = dir.path().join("credentials");
    std::fs::write(&credentials, "https://stored:secret@example.test\n").expect("credentials");
    std::fs::write(
        repo.git_dir.join("config"),
        format!(
            "[credential]\n\thelper = store --file {}\n",
            credentials.display()
        ),
    )
    .expect("config");

    let url = parsed_http_url_with_extra_headers(
        Some(&repo),
        "https://url-user:url-pass@example.test/repo.git",
    )
    .expect("parsed URL with credential helper");
"#;
        let ops = diff_line_ops_myers(&split_diff_lines(old), &split_diff_lines(new));
        assert_eq!(
            diff_line_op_tags(&ops),
            vec![
                (
                    b' ',
                    b"    assert_eq!(url.authorization.as_deref(), Some(\"Basic dXNlcjpwQHNz\"));\n"
                        .to_vec(),
                ),
                (b' ', b"}\n".to_vec()),
                (b' ', b"\n".to_vec()),
                (b'+', b"#[test]\n".to_vec()),
                (
                    b'+',
                    b"fn parsed_http_url_reads_credential_store_helper_basic_auth_with_quoted_file_path() {\n"
                        .to_vec(),
                ),
                (b'+', b"    let dir = tempfile::TempDir::new().expect(\"repo\");\n".to_vec()),
                (b'+', b"    let repo = test_repo_at(dir.path());\n".to_vec()),
                (
                    b'+',
                    b"    let credentials_dir = dir.path().join(\"folder with spaces\");\n".to_vec(),
                ),
                (
                    b'+',
                    b"    std::fs::create_dir_all(&credentials_dir).expect(\"credentials dir\");\n"
                        .to_vec(),
                ),
                (
                    b'+',
                    b"    let credentials = credentials_dir.join(\"quoted credentials\");\n".to_vec(),
                ),
                (
                    b'+',
                    b"    std::fs::write(&credentials, \"https://user:p%40ss@example.test\\n\").expect(\"credentials\");\n"
                        .to_vec(),
                ),
                (b'+', b"    std::fs::write(\n".to_vec()),
                (b'+', b"        repo.git_dir.join(\"config\"),\n".to_vec()),
                (b'+', b"        format!(\n".to_vec()),
                (
                    b'+',
                    b"            \"[credential]\\n\\thelper = store --file '{}'\\n\",\n".to_vec(),
                ),
                (b'+', b"            credentials.display()\n".to_vec()),
                (b'+', b"        ),\n".to_vec()),
                (b'+', b"    )\n".to_vec()),
                (b'+', b"    .expect(\"config\");\n".to_vec()),
                (b'+', b"\n".to_vec()),
                (
                    b'+',
                    b"    let url = parsed_http_url_with_extra_headers(Some(&repo), \"https://example.test/repo.git\")\n"
                        .to_vec(),
                ),
                (
                    b'+',
                    b"        .expect(\"parsed URL with credential helper\");\n".to_vec(),
                ),
                (b'+', b"\n".to_vec()),
                (
                    b'+',
                    b"    assert_eq!(url.authorization.as_deref(), Some(\"Basic dXNlcjpwQHNz\"));\n"
                        .to_vec(),
                ),
                (b'+', b"}\n".to_vec()),
                (b'+', b"\n".to_vec()),
                (b' ', b"#[test]\n".to_vec()),
                (
                    b' ',
                    b"fn parsed_http_url_keeps_url_userinfo_before_credential_store() {\n".to_vec(),
                ),
                (b' ', b"    let dir = tempfile::TempDir::new().expect(\"repo\");\n".to_vec()),
                (b' ', b"    let repo = test_repo_at(dir.path());\n".to_vec()),
                (b' ', b"    let credentials = dir.path().join(\"credentials\");\n".to_vec()),
                (
                    b' ',
                    b"    std::fs::write(&credentials, \"https://stored:secret@example.test\\n\").expect(\"credentials\");\n"
                        .to_vec(),
                ),
                (b' ', b"    std::fs::write(\n".to_vec()),
                (b' ', b"        repo.git_dir.join(\"config\"),\n".to_vec()),
                (b' ', b"        format!(\n".to_vec()),
                (
                    b' ',
                    b"            \"[credential]\\n\\thelper = store --file {}\\n\",\n".to_vec(),
                ),
                (b' ', b"            credentials.display()\n".to_vec()),
                (b' ', b"        ),\n".to_vec()),
                (b' ', b"    )\n".to_vec()),
                (b' ', b"    .expect(\"config\");\n".to_vec()),
                (b' ', b"\n".to_vec()),
                (
                    b' ',
                    b"    let url = parsed_http_url_with_extra_headers(\n".to_vec(),
                ),
                (b' ', b"        Some(&repo),\n".to_vec()),
                (
                    b' ',
                    b"        \"https://url-user:url-pass@example.test/repo.git\",\n".to_vec(),
                ),
                (
                    b' ',
                    b"    )\n".to_vec(),
                ),
                (
                    b' ',
                    b"    .expect(\"parsed URL with credential helper\");\n".to_vec(),
                ),
            ]
        );
    }

    #[test]
    #[ignore = "debug render_ops for transport quoted credential test parity"]
    fn debug_render_ops_transport_inserted_test() {
        let old = br#"    assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
}

#[test]
fn parsed_http_url_keeps_url_userinfo_before_credential_store() {
    let dir = tempfile::TempDir::new().expect("repo");
    let repo = test_repo_at(dir.path());
    let credentials = dir.path().join("credentials");
    std::fs::write(&credentials, "https://stored:secret@example.test\n").expect("credentials");
    std::fs::write(
        repo.git_dir.join("config"),
        format!(
            "[credential]\n\thelper = store --file {}\n",
            credentials.display()
        ),
    )
    .expect("config");

    let url = parsed_http_url_with_extra_headers(
        Some(&repo),
        "https://url-user:url-pass@example.test/repo.git",
    )
    .expect("parsed URL with credential helper");
"#;
        let new = br#"    assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
}

#[test]
fn parsed_http_url_reads_credential_store_helper_basic_auth_with_quoted_file_path() {
    let dir = tempfile::TempDir::new().expect("repo");
    let repo = test_repo_at(dir.path());
    let credentials_dir = dir.path().join("folder with spaces");
    std::fs::create_dir_all(&credentials_dir).expect("credentials dir");
    let credentials = credentials_dir.join("quoted credentials");
    std::fs::write(&credentials, "https://user:p%40ss@example.test\n").expect("credentials");
    std::fs::write(
        repo.git_dir.join("config"),
        format!(
            "[credential]\n\thelper = store --file '{}'\n",
            credentials.display()
        ),
    )
    .expect("config");

    let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
        .expect("parsed URL with credential helper");

    assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
}

#[test]
fn parsed_http_url_keeps_url_userinfo_before_credential_store() {
    let dir = tempfile::TempDir::new().expect("repo");
    let repo = test_repo_at(dir.path());
    let credentials = dir.path().join("credentials");
    std::fs::write(&credentials, "https://stored:secret@example.test\n").expect("credentials");
    std::fs::write(
        repo.git_dir.join("config"),
        format!(
            "[credential]\n\thelper = store --file {}\n",
            credentials.display()
        ),
    )
    .expect("config");

    let url = parsed_http_url_with_extra_headers(
        Some(&repo),
        "https://url-user:url-pass@example.test/repo.git",
    )
    .expect("parsed URL with credential helper");
"#;
        panic!("{:?}", rendered_hunk_op_tags(old, new));
    }

    #[test]
    #[ignore = "debug exact observed transport fragment from HEAD~1..HEAD"]
    fn debug_exact_observed_transport_fragment_hunk() {
        let old = br#"        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with credential helper");

        assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
    }

    #[test]
    fn parsed_http_url_keeps_url_userinfo_before_credential_store() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        let credentials = dir.path().join("credentials");
        std::fs::write(&credentials, "https://stored:secret@example.test\n").expect("credentials");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file {}\n",
                credentials.display()
            ),
        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(
            Some(&repo),
            "https://url-user:url-pass@example.test/repo.git",
        )
        .expect("parsed URL with credential helper");

        assert_eq!(
            url.authorization.as_deref(),
            Some("Basic dXJsLXVzZXI6dXJsLXBhc3M=")
        );
    }
"#;
        let new = br#"        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with credential helper");

        assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
    }

    #[test]
    fn parsed_http_url_reads_credential_store_helper_basic_auth_with_quoted_file_path() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        let credentials_dir = dir.path().join("folder with spaces");
        std::fs::create_dir_all(&credentials_dir).expect("credentials dir");
        let credentials = credentials_dir.join("quoted credentials");
        std::fs::write(&credentials, "https://user:p%40ss@example.test\n").expect("credentials");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file '{}'\n",
                credentials.display()
            ),
        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with credential helper");

        assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
    }

    #[test]
    fn parsed_http_url_keeps_url_userinfo_before_credential_store() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        let credentials = dir.path().join("credentials");
        std::fs::write(&credentials, "https://stored:secret@example.test\n").expect("credentials");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file {}\n",
                credentials.display()
            ),
        )
        .expect("config");
"#;
        let mut out = Vec::new();
        write_unified_full_file_hunk(
            &mut out,
            old,
            new,
            "transport_impl.rs",
            HunkFormatOptions::default(),
        )
        .expect("write exact transport fragment hunk");
        panic!("{}", String::from_utf8(out).expect("utf8"));
    }

    #[test]
    #[ignore = "debug exact observed transport fragment ops from HEAD~1..HEAD"]
    fn debug_exact_observed_transport_fragment_ops() {
        let old = br#"        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with credential helper");

        assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
    }

    #[test]
    fn parsed_http_url_keeps_url_userinfo_before_credential_store() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        let credentials = dir.path().join("credentials");
        std::fs::write(&credentials, "https://stored:secret@example.test\n").expect("credentials");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file {}\n",
                credentials.display()
            ),
        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(
            Some(&repo),
            "https://url-user:url-pass@example.test/repo.git",
        )
        .expect("parsed URL with credential helper");

        assert_eq!(
            url.authorization.as_deref(),
            Some("Basic dXJsLXVzZXI6dXJsLXBhc3M=")
        );
    }
"#;
        let new = br#"        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with credential helper");

        assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
    }

    #[test]
    fn parsed_http_url_reads_credential_store_helper_basic_auth_with_quoted_file_path() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        let credentials_dir = dir.path().join("folder with spaces");
        std::fs::create_dir_all(&credentials_dir).expect("credentials dir");
        let credentials = credentials_dir.join("quoted credentials");
        std::fs::write(&credentials, "https://user:p%40ss@example.test\n").expect("credentials");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file '{}'\n",
                credentials.display()
            ),
        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with credential helper");

        assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
    }

    #[test]
    fn parsed_http_url_keeps_url_userinfo_before_credential_store() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        let credentials = dir.path().join("credentials");
        std::fs::write(&credentials, "https://stored:secret@example.test\n").expect("credentials");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file {}\n",
                credentials.display()
            ),
        )
"#;
        panic!("{:?}", debug_line_tags(old, new));
    }

    #[test]
    fn stat_graph_matches_git_ratio_rounding_for_mixed_changes() {
        assert_eq!(stat_graph(541, 102, 643, 25), "+++++++++++++++++++++----");
    }

    #[test]
    fn unified_hunk_keeps_equal_function_body_after_collapsed_signature_change() {
        let old = br#"fn example(
    a: i32,
    b: i32,
) -> i32 {
    entries
        .iter()
        .filter(|entry| *entry > 0)
        .count() as i32
}

fn second() {
    println!("old");
}
"#;
        let new = br#"fn example(a: i32, b: i32) -> i32 {
    entries
        .iter()
        .filter(|entry| *entry > 0)
        .count() as i32
}

fn second() {
    println!("new");
}
"#;

        let mut out = Vec::new();
        write_unified_full_file_hunk(&mut out, old, new, "f.rs", HunkFormatOptions::default())
            .expect("write unified hunk");

        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            concat!(
                "@@ -1,7 +1,4 @@\n",
                "-fn example(\n",
                "-    a: i32,\n",
                "-    b: i32,\n",
                "-) -> i32 {\n",
                "+fn example(a: i32, b: i32) -> i32 {\n",
                "     entries\n",
                "         .iter()\n",
                "         .filter(|entry| *entry > 0)\n",
                "@@ -9,5 +6,5 @@ fn example(\n",
                " }\n",
                " \n",
                " fn second() {\n",
                "-    println!(\"old\");\n",
                "+    println!(\"new\");\n",
                " }\n",
            )
        );
    }

    #[test]
    fn stat_graph_matches_git_ratio_rounding_for_insert_only_rows() {
        assert_eq!(stat_graph(460, 0, 643, 21), "+++++++++++++++");
        assert_eq!(stat_graph(347, 0, 643, 21), "+++++++++++");
        assert_eq!(stat_graph(216, 0, 643, 21), "+++++++");
    }

    #[test]
    fn stat_graph_keeps_both_markers_for_small_mixed_rows() {
        assert_eq!(stat_graph(1, 17, 643, 21), "+-");
    }

    #[test]
    fn diff_stat_row_mode_only_same_blob_does_not_read_object_content() {
        let dir = TempDir::new().expect("temp dir");
        let root = dir.path().join("repo");
        let git_dir = root.join(".git");
        let objects_dir = git_dir.join("objects");
        fs::create_dir_all(&objects_dir).expect("create objects dir");
        let repo = GitRepo {
            root: root.clone(),
            git_dir: git_dir.clone(),
            objects_dir: objects_dir.clone(),
            index_path: git_dir.join("index"),
        };
        let store = LooseObjectStore::new(&objects_dir, GitHashAlgorithm::Sha1);
        let blob_id = ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            "1111111111111111111111111111111111111111",
        )
        .expect("blob id");
        let old_index = GitIndex::from_entries(vec![
            IndexEntry::new("mode-only.txt", blob_id.clone(), IndexMode::File, 12)
                .expect("old entry"),
        ])
        .expect("old index");
        let new_index = GitIndex::from_entries(vec![
            IndexEntry::new("mode-only.txt", blob_id, IndexMode::Executable, 12)
                .expect("new entry"),
        ])
        .expect("new index");
        let context = DiffIndexContext {
            repo: &repo,
            store: &store,
            old_index: &old_index,
            new_index: &new_index,
            old_source: DiffSideSource::Index,
            new_source: DiffSideSource::Index,
        };
        let entry = IndexDiffEntry {
            status: IndexDiffStatus::Modified,
            path: b"mode-only.txt".to_vec(),
            old_path: None,
            similarity: None,
        };

        let row = diff_stat_row_with_whitespace(
            &context,
            &entry,
            DiffStatOptions {
                whitespace_mode: DiffWhitespaceMode::None,
                relative_prefix: None,
                ignore_matching_lines: &[],
                ignore_blank_lines: false,
                compact_summary: false,
                color: false,
            },
            None,
            false,
        )
        .expect("mode-only row");

        assert_eq!(row.insertions, 0);
        assert_eq!(row.deletions, 0);
        assert_eq!(row.old_bytes, 12);
        assert_eq!(row.new_bytes, 12);
        assert!(!row.binary);
    }

    #[test]
    fn diff_stat_single_sided_counts_match_ignore_hunk_semantics() {
        let ignore_debug = [Regex::new("DEBUG").expect("debug regex")];

        assert_eq!(
            diff_stat_single_sided_counts(b"DEBUG one\nDEBUG two\n", true, &ignore_debug, false),
            (0, 0)
        );
        assert_eq!(
            diff_stat_single_sided_counts(b"DEBUG one\nrelease line\n", true, &ignore_debug, false),
            (2, 0)
        );
        assert_eq!(
            diff_stat_single_sided_counts(b"\n \n\t\n", false, &[], true),
            (0, 0)
        );
        assert_eq!(
            diff_stat_single_sided_counts(b"\nrelease line\n", false, &[], true),
            (0, 2)
        );
    }

    #[test]
    fn diff_stat_binary_and_line_count_matches_existing_helpers() {
        for content in [
            b"".as_slice(),
            b"one line\n",
            b"one\n\ntwo",
            b"\0binary prefix\nstill here\n",
            b"unterminated",
        ] {
            assert_eq!(
                diff_stat_binary_and_line_count(content),
                (is_binary_content(content), count_diff_lines(content))
            );
        }
    }

    #[test]
    fn diff_line_counts_plain_does_not_treat_same_line_prefix_growth_as_pure_insertion() {
        assert_eq!(diff_line_counts_plain(b"staged\n", b"unstaged\n"), (1, 1));
        assert_eq!(diff_line_counts_plain(b"unstaged\n", b"staged\n"), (1, 1));
    }

    #[test]
    fn format_patch_output_filename_preserves_reroll_prefix_when_truncating_slug() {
        let dir = TempDir::new().expect("temp dir");
        let repo = test_repo(dir.path());
        let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
        let notes = HashMap::new();
        let context = FormatPatchContext {
            repo: &repo,
            store: &store,
            abbrev_len: 7,
            patch_abbrev_len: 7,
            total: 3,
            nul_terminated: false,
            no_prefix: false,
            no_numbered: false,
            numbered: false,
            numbered_files: false,
            attach: false,
            inline: false,
            cover_letter: true,
            include_mime_headers: false,
            mime_boundary: None,
            mboxrd: false,
            suffix: ".patch",
            subject_prefix: "PATCH",
            reroll_count: Some("4---..././../--1/.2//"),
            commit_list_format: None,
            prelude_mode: FormatPatchPreludeMode::Diffstat,
            reverse: false,
            order_file: None,
            skip_to: None,
            rotate_to: None,
            word_diff: WordDiffMode::None,
            word_diff_regex: None,
            submodule_format: SubmoduleDiffFormat::Short,
            unified_context: 3,
            thread: None,
            extra_headers: &[],
            in_reply_to: None,
            sender_override: None,
            body_from_override: false,
            encode_email_headers: true,
            message_id_timestamp: None,
            notes_by_commit: &notes,
            keep_subject: false,
            number_offset: 0,
            filename_max_length: Some(64),
            signoff_line: None,
            signature: None,
            zero_commit: false,
            cover_subject: None,
            cover_blurb: None,
            base_information: None,
            appendix: None,
            relative_prefix: None,
            pathspecs: &[],
            rename_threshold: Some(100),
            copy_threshold: None,
            find_copies_harder: false,
        };

        let filename = format_patch_output_filename(
            3,
            "Side changes #3 with \\n backslash-n in it.",
            &context,
        );

        assert_eq!(
            filename,
            "v4-.-.-.-1-.2-0003-Side-changes-3-with-n-backslash-n-in-i.patch"
        );
    }
}
