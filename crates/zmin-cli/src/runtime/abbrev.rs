use std::path::Path;

use zmin_git_core::{GitHashAlgorithm, GitObjectStore, LooseObjectStore, ObjectId};

use super::{CliError, ConfigEntry, ConfigScope, GitRepo, Result, read_config_entries};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreAbbrevConfigState {
    Auto,
    Full,
    Minimum(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AbbrevAction<'a> {
    Default,
    Bare,
    Explicit(&'a str),
    NoAbbrev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShowRefOutputOption {
    Hash(Option<usize>),
    Abbrev(Option<usize>),
    NoAbbrev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShowRefOutputPolicy {
    pub(crate) hash_output: bool,
    pub(crate) abbreviation: CoreAbbrevConfigState,
}

pub(crate) fn resolve_show_ref_output_policy(
    configured: CoreAbbrevConfigState,
    options: &[ShowRefOutputOption],
    algorithm: GitHashAlgorithm,
) -> ShowRefOutputPolicy {
    let full = full_abbrev_len(algorithm);
    let mut policy = ShowRefOutputPolicy {
        hash_output: false,
        abbreviation: CoreAbbrevConfigState::Full,
    };
    for option in options {
        match *option {
            ShowRefOutputOption::Hash(value) => {
                policy.hash_output = true;
                policy.abbreviation = match value {
                    None | Some(0) => CoreAbbrevConfigState::Full,
                    Some(value) => CoreAbbrevConfigState::Minimum(value.max(4).min(full)),
                };
            }
            ShowRefOutputOption::Abbrev(value) => {
                policy.abbreviation = match value {
                    None => configured,
                    Some(0) => CoreAbbrevConfigState::Full,
                    Some(value) => CoreAbbrevConfigState::Minimum(value.max(4).min(full)),
                };
            }
            ShowRefOutputOption::NoAbbrev => {
                policy.abbreviation = CoreAbbrevConfigState::Full;
            }
        }
    }
    policy
}

pub(crate) fn resolve_abbrev_action<'a>(
    abbrev: Option<&'a str>,
    no_abbrev: bool,
) -> AbbrevAction<'a> {
    if no_abbrev {
        AbbrevAction::NoAbbrev
    } else if let Some(value) = abbrev {
        if value.is_empty() {
            AbbrevAction::Bare
        } else {
            AbbrevAction::Explicit(value)
        }
    } else {
        AbbrevAction::Default
    }
}

pub(crate) fn policy_for_abbrev_action(
    configured: CoreAbbrevConfigState,
    action: AbbrevAction<'_>,
    algorithm: GitHashAlgorithm,
) -> CoreAbbrevConfigState {
    match action {
        AbbrevAction::Default | AbbrevAction::Bare => configured,
        AbbrevAction::NoAbbrev => CoreAbbrevConfigState::Full,
        AbbrevAction::Explicit(value) => {
            CoreAbbrevConfigState::Minimum(parse_revision_abbrev(value, algorithm))
        }
    }
}

pub(crate) fn parse_git_config_integer(value: &str) -> std::result::Result<i32, &'static str> {
    let value = value.trim_start_matches(|character: char| {
        matches!(
            character,
            ' ' | '\t' | '\n' | '\r' | '\u{000b}' | '\u{000c}'
        )
    });
    if value.is_empty() {
        return Err("invalid unit");
    }
    let mut number = value;
    let multiplier = match number.as_bytes().last().copied() {
        Some(b'k' | b'K') => {
            number = &number[..number.len() - 1];
            1024_i128
        }
        Some(b'm' | b'M') => {
            number = &number[..number.len() - 1];
            1024_i128 * 1024
        }
        Some(b'g' | b'G') => {
            number = &number[..number.len() - 1];
            1024_i128 * 1024 * 1024
        }
        Some(byte) if byte.is_ascii_digit() => 1,
        _ => return Err("invalid unit"),
    };
    if number.is_empty() || number == "+" || number == "-" {
        return Err("invalid unit");
    }
    let (negative, digits) = match number.as_bytes().first().copied() {
        Some(b'-') => (true, &number[1..]),
        Some(b'+') => (false, &number[1..]),
        _ => (false, number),
    };
    if digits.is_empty() {
        return Err("invalid unit");
    }
    let (radix, digits) = if let Some(rest) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, rest)
    } else if digits.len() > 1 && digits.starts_with('0') {
        (8, &digits[1..])
    } else {
        (10, digits)
    };
    if digits.is_empty() {
        return Err("invalid unit");
    }
    let valid_digit = |byte: u8| match radix {
        8 => (b'0'..=b'7').contains(&byte),
        10 => byte.is_ascii_digit(),
        16 => byte.is_ascii_hexdigit(),
        _ => false,
    };
    if !digits.bytes().all(valid_digit) {
        return Err("invalid unit");
    }
    let magnitude = u128::from_str_radix(digits, radix).map_err(|_| "out of range")?;
    let magnitude = i128::try_from(magnitude).map_err(|_| "out of range")?;
    let signed = if negative {
        magnitude.checked_neg().ok_or("out of range")?
    } else {
        magnitude
    };
    let scaled = signed.checked_mul(multiplier).ok_or("out of range")?;
    i32::try_from(scaled).map_err(|_| "out of range")
}

pub(crate) fn parse_revision_abbrev(value: &str, algorithm: GitHashAlgorithm) -> usize {
    const MINIMUM_ABBREV: usize = 4;
    let full_width = full_abbrev_len(algorithm);
    let value = value.trim_start_matches(|character: char| {
        matches!(
            character,
            ' ' | '\t' | '\n' | '\r' | '\u{000b}' | '\u{000c}'
        )
    });
    let (negative, digits) = match value.as_bytes().first().copied() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    let unsigned_long_bits = (std::mem::size_of::<std::os::raw::c_ulong>() * 8) as u32;
    let unsigned_long_max = if unsigned_long_bits == 128 {
        u128::MAX
    } else {
        (1_u128 << unsigned_long_bits) - 1
    };
    let mut magnitude = 0_u128;
    let mut saw_digit = false;
    for byte in digits.bytes() {
        if !byte.is_ascii_digit() {
            break;
        }
        saw_digit = true;
        let digit = u128::from(byte - b'0');
        if magnitude > (unsigned_long_max - digit) / 10 {
            return full_width;
        }
        magnitude = magnitude * 10 + digit;
    }
    let parsed = if negative {
        (0_u128.wrapping_sub(magnitude)) & unsigned_long_max
    } else {
        magnitude
    };
    if !saw_digit || parsed < MINIMUM_ABBREV as u128 {
        MINIMUM_ABBREV
    } else {
        parsed.min(full_width as u128) as usize
    }
}

pub(crate) fn configured_core_abbrev_state(repo: &GitRepo) -> Result<CoreAbbrevConfigState> {
    let entries = read_config_entries(repo)?;
    resolve_core_abbrev_config_state(Some(repo), &entries)
}

pub(crate) fn resolve_core_abbrev_config_state(
    repo: Option<&GitRepo>,
    entries: &[ConfigEntry],
) -> Result<CoreAbbrevConfigState> {
    let mut state = CoreAbbrevConfigState::Auto;
    for entry in entries.iter().filter(|entry| {
        entry.section == "core" && entry.subsection.is_empty() && entry.key == "abbrev"
    }) {
        state = resolve_core_abbrev_config_entry(repo, entry)?;
    }
    Ok(state)
}

fn resolve_core_abbrev_config_entry(
    repo: Option<&GitRepo>,
    entry: &ConfigEntry,
) -> Result<CoreAbbrevConfigState> {
    const MINIMUM_ABBREV: i32 = 4;

    if entry.implicit_bool {
        return Err(core_abbrev_missing_config_value(repo, entry));
    }
    let value = entry.value.as_str();
    if value.eq_ignore_ascii_case("auto") {
        return Ok(CoreAbbrevConfigState::Auto);
    }
    if value.is_empty()
        || value.eq_ignore_ascii_case("no")
        || value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("off")
    {
        return Ok(CoreAbbrevConfigState::Full);
    }
    let parsed = parse_git_config_integer(value)
        .map_err(|reason| core_abbrev_bad_numeric_config(repo, entry, reason))?;
    if parsed < MINIMUM_ABBREV {
        return Err(core_abbrev_abbrev_out_of_range(
            repo,
            entry,
            i64::from(parsed),
        ));
    }
    Ok(CoreAbbrevConfigState::Minimum(parsed as usize))
}

fn core_abbrev_config_file(repo: Option<&GitRepo>, entry: &ConfigEntry) -> String {
    let Some(path) = entry.origin.strip_prefix("file:") else {
        return entry.origin.clone();
    };
    if repo.is_some_and(|repo| Path::new(path) == repo.git_dir.join("config")) {
        ".git/config".to_owned()
    } else {
        path.to_owned()
    }
}

fn core_abbrev_missing_config_value(repo: Option<&GitRepo>, entry: &ConfigEntry) -> CliError {
    let text = if entry.scope == ConfigScope::Command {
        "error: missing value for 'core.abbrev'\nfatal: unable to parse 'core.abbrev' from command-line config\n".to_owned()
    } else {
        format!(
            "error: missing value for 'core.abbrev'\nfatal: bad config variable 'core.abbrev' in file '{}' at line {}\n",
            core_abbrev_config_file(repo, entry),
            entry.line.unwrap_or_default()
        )
    };
    CliError::Stderr { code: 128, text }
}

fn core_abbrev_bad_numeric_config(
    repo: Option<&GitRepo>,
    entry: &ConfigEntry,
    reason: &str,
) -> CliError {
    let message = if entry.scope == ConfigScope::Command {
        format!(
            "bad numeric config value '{}' for 'core.abbrev': {reason}",
            entry.value
        )
    } else {
        format!(
            "bad numeric config value '{}' for 'core.abbrev' in file {}: {reason}",
            entry.value,
            core_abbrev_config_file(repo, entry)
        )
    };
    CliError::Fatal { code: 128, message }
}

fn core_abbrev_abbrev_out_of_range(
    repo: Option<&GitRepo>,
    entry: &ConfigEntry,
    value: i64,
) -> CliError {
    let text = if entry.scope == ConfigScope::Command {
        format!(
            "error: abbrev length out of range: {value}\nfatal: unable to parse 'core.abbrev' from command-line config\n"
        )
    } else {
        format!(
            "error: abbrev length out of range: {value}\nfatal: bad config variable 'core.abbrev' in file '{}' at line {}\n",
            core_abbrev_config_file(repo, entry),
            entry.line.unwrap_or_default()
        )
    };
    CliError::Stderr { code: 128, text }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedAbbrevLengths {
    entries: Vec<RenderedAbbrevLengthEntry>,
    empty_width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedAbbrevLengthEntry {
    id: ObjectId,
    width: usize,
}

impl RenderedAbbrevLengths {
    pub(crate) fn fixed(ids: &[ObjectId], width: usize) -> Self {
        Self::from_ids_and_widths(ids, vec![width; ids.len()], width)
    }

    fn from_ids_and_widths(ids: &[ObjectId], widths: Vec<usize>, empty_width: usize) -> Self {
        let mut entries = ids
            .iter()
            .cloned()
            .zip(widths)
            .map(|(id, width)| RenderedAbbrevLengthEntry { id, width })
            .collect::<Vec<_>>();
        entries.sort_unstable_by(|left, right| left.id.as_bytes().cmp(right.id.as_bytes()));
        let mut unique_entries: Vec<RenderedAbbrevLengthEntry> = Vec::with_capacity(entries.len());
        for entry in entries {
            if let Some(previous) = unique_entries.last_mut() {
                if previous.id == entry.id {
                    previous.width = previous.width.max(entry.width);
                    continue;
                }
            }
            unique_entries.push(entry);
        }
        Self {
            entries: unique_entries,
            empty_width,
        }
    }

    pub(crate) fn from_store_minimum(
        store: &LooseObjectStore,
        ids: &[ObjectId],
        minimum: usize,
    ) -> Result<Self> {
        let widths = store
            .minimum_unique_abbrev_lens_for_ids(ids, minimum)
            .map_err(CliError::Io)?;
        Ok(Self::from_ids_and_widths(
            ids,
            widths,
            minimum.min(full_abbrev_len(store.algorithm())),
        ))
    }

    pub(crate) fn from_ids_minimum(
        ids: &[ObjectId],
        minimum: usize,
        algorithm: GitHashAlgorithm,
    ) -> Self {
        let full_width = full_abbrev_len(algorithm);
        let minimum = minimum.min(full_width);
        if ids.is_empty() {
            return Self::fixed(ids, minimum);
        }
        let mut unique_ids = ids.to_vec();
        unique_ids.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        unique_ids.dedup();
        let mut widths = vec![minimum; unique_ids.len()];
        for index in 1..unique_ids.len() {
            let common = common_hex_prefix_len(&unique_ids[index - 1], &unique_ids[index]);
            let width = common.saturating_add(1).min(full_width);
            widths[index - 1] = widths[index - 1].max(width);
            widths[index] = widths[index].max(width);
        }
        Self::from_ids_and_widths(&unique_ids, widths, minimum)
    }

    pub(crate) fn width_for(&self, id: &ObjectId) -> usize {
        self.entries
            .binary_search_by(|entry| entry.id.as_bytes().cmp(id.as_bytes()))
            .map(|index| self.entries[index].width)
            .unwrap_or(self.empty_width)
    }

    pub(crate) fn empty_width(&self) -> usize {
        self.empty_width
    }
}

fn common_hex_prefix_len(left: &ObjectId, right: &ObjectId) -> usize {
    for (byte_index, (left_byte, right_byte)) in
        left.as_bytes().iter().zip(right.as_bytes()).enumerate()
    {
        if left_byte == right_byte {
            continue;
        }
        return byte_index * 2
            + if left_byte >> 4 == right_byte >> 4 {
                1
            } else {
                0
            };
    }
    left.as_bytes().len() * 2
}

pub(crate) fn rendered_abbrev_len_for_ids(
    store: &LooseObjectStore,
    policy: CoreAbbrevConfigState,
    ids: &[ObjectId],
) -> Result<RenderedAbbrevLengths> {
    let full_width = full_abbrev_len(store.algorithm());
    match policy {
        CoreAbbrevConfigState::Full => Ok(RenderedAbbrevLengths::fixed(ids, full_width)),
        CoreAbbrevConfigState::Auto => {
            if ids.is_empty() {
                Ok(RenderedAbbrevLengths::fixed(ids, full_width))
            } else {
                let widths = store
                    .minimum_unique_abbrev_lens_for_ids(ids, auto_abbrev_len_for_store(store)?)
                    .map_err(CliError::Io)?;
                Ok(RenderedAbbrevLengths::from_ids_and_widths(
                    ids, widths, full_width,
                ))
            }
        }
        CoreAbbrevConfigState::Minimum(minimum) => {
            if ids.is_empty() {
                Ok(RenderedAbbrevLengths::fixed(ids, minimum.min(full_width)))
            } else {
                let widths = store
                    .minimum_unique_abbrev_lens_for_ids(ids, minimum)
                    .map_err(CliError::Io)?;
                Ok(RenderedAbbrevLengths::from_ids_and_widths(
                    ids,
                    widths,
                    minimum.min(full_width),
                ))
            }
        }
    }
}

pub(crate) fn auto_abbrev_len_from_object_count(object_count: usize) -> usize {
    const MIN_ABBREV: usize = 7;
    if object_count == 0 {
        return MIN_ABBREV;
    }
    let squared = (object_count as u128).saturating_mul(object_count as u128);
    let hex_digits = ((u128::BITS as usize) - squared.leading_zeros() as usize).div_ceil(4);
    MIN_ABBREV.max(hex_digits)
}

pub(crate) fn auto_abbrev_len_for_store(store: &impl GitObjectStore) -> Result<usize> {
    Ok(auto_abbrev_len_from_object_count(
        store.object_id_capacity_hint()?,
    ))
}

pub(crate) fn full_abbrev_len(algorithm: GitHashAlgorithm) -> usize {
    algorithm.digest_len() * 2
}

pub(crate) struct BorrowedObjectIds<'a> {
    ids: Vec<&'a ObjectId>,
}

impl<'a> BorrowedObjectIds<'a> {
    pub(crate) fn from_iter<I>(ids: I) -> Self
    where
        I: IntoIterator<Item = &'a ObjectId>,
    {
        Self {
            ids: ids.into_iter().collect(),
        }
    }

    fn len(&self) -> usize {
        self.ids.len()
    }

    fn get(&self, index: usize) -> &ObjectId {
        self.ids[index]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedAbbrevWidths {
    widths: Vec<usize>,
    empty_width: usize,
}

impl RenderedAbbrevWidths {
    pub(crate) fn fixed(row_count: usize, width: usize) -> Self {
        Self {
            widths: vec![width; row_count],
            empty_width: width,
        }
    }

    pub(crate) fn width_for(&self, row_index: usize) -> usize {
        self.widths
            .get(row_index)
            .copied()
            .unwrap_or(self.empty_width)
    }

    pub(crate) fn empty_width(&self) -> usize {
        self.empty_width
    }
}

pub(crate) fn rendered_abbrev_widths_for_borrowed_ids(
    store: &LooseObjectStore,
    policy: CoreAbbrevConfigState,
    ids: &BorrowedObjectIds<'_>,
) -> Result<RenderedAbbrevWidths> {
    let full_width = full_abbrev_len(store.algorithm());
    match policy {
        CoreAbbrevConfigState::Full => Ok(RenderedAbbrevWidths::fixed(ids.len(), full_width)),
        CoreAbbrevConfigState::Auto => {
            if ids.len() == 0 {
                return Ok(RenderedAbbrevWidths::fixed(0, full_width));
            }
            let minimum = auto_abbrev_len_for_store(store)?;
            let widths = minimum_unique_borrowed_abbrev_widths(store, ids, minimum)?;
            Ok(RenderedAbbrevWidths {
                widths,
                empty_width: full_width,
            })
        }
        CoreAbbrevConfigState::Minimum(minimum) => {
            let minimum = minimum.min(full_width);
            if ids.len() == 0 || minimum == full_width {
                return Ok(RenderedAbbrevWidths::fixed(ids.len(), minimum));
            }
            let widths = minimum_unique_borrowed_abbrev_widths(store, ids, minimum)?;
            Ok(RenderedAbbrevWidths {
                widths,
                empty_width: minimum,
            })
        }
    }
}

fn minimum_unique_borrowed_abbrev_widths(
    store: &LooseObjectStore,
    ids: &BorrowedObjectIds<'_>,
    minimum: usize,
) -> Result<Vec<usize>> {
    let full_width = full_abbrev_len(store.algorithm());
    if ids.len() == 0 {
        return Ok(Vec::new());
    }
    let mut sorted_indices = (0..ids.len()).collect::<Vec<_>>();
    sorted_indices.sort_unstable_by(|left, right| {
        ids.get(*left)
            .as_bytes()
            .cmp(ids.get(*right).as_bytes())
            .then_with(|| left.cmp(right))
    });
    let mut unique_indices = Vec::with_capacity(sorted_indices.len());
    for &index in &sorted_indices {
        if unique_indices
            .last()
            .is_none_or(|previous| ids.get(*previous) != ids.get(index))
        {
            unique_indices.push(index);
        }
    }
    let mut unique_widths = vec![minimum; unique_indices.len()];
    for (index, pair) in unique_indices.windows(2).enumerate() {
        let [left, right] = pair else {
            continue;
        };
        let width = common_hex_prefix_len(ids.get(*left), ids.get(*right))
            .saturating_add(1)
            .min(full_width);
        unique_widths[index] = unique_widths[index].max(width);
        unique_widths[index + 1] = unique_widths[index + 1].max(width);
    }
    store
        .for_each_object_id(&mut |candidate| {
            let insertion = match unique_indices
                .binary_search_by(|index| ids.get(*index).as_bytes().cmp(candidate.as_bytes()))
            {
                Ok(_) => return Ok(()),
                Err(insertion) => insertion,
            };
            if let Some(index) = insertion.checked_sub(1) {
                unique_widths[index] = unique_widths[index].max(
                    common_hex_prefix_len(ids.get(unique_indices[index]), candidate)
                        .saturating_add(1)
                        .min(full_width),
                );
            }
            if let Some(index) = unique_widths.get_mut(insertion) {
                *index = (*index).max(
                    common_hex_prefix_len(ids.get(unique_indices[insertion]), candidate)
                        .saturating_add(1)
                        .min(full_width),
                );
            }
            Ok(())
        })
        .map_err(CliError::Io)?;
    let mut widths = vec![minimum; ids.len()];
    let mut sorted_cursor = 0;
    for (unique_index, &representative) in unique_indices.iter().enumerate() {
        let target = ids.get(representative);
        while sorted_cursor < sorted_indices.len()
            && ids.get(sorted_indices[sorted_cursor]) == target
        {
            widths[sorted_indices[sorted_cursor]] = unique_widths[unique_index];
            sorted_cursor += 1;
        }
    }
    Ok(widths)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn fake_loose_object(objects_dir: &std::path::Path, hex: &str) -> ObjectId {
        fs::create_dir_all(objects_dir.join(&hex[..2])).expect("create fanout");
        fs::write(objects_dir.join(&hex[..2]).join(&hex[2..]), b"").expect("write object");
        ObjectId::from_hex(
            if hex.len() == 40 {
                GitHashAlgorithm::Sha1
            } else {
                GitHashAlgorithm::Sha256
            },
            hex,
        )
        .expect("object id")
    }

    #[test]
    fn rendered_abbrev_lengths_preserve_per_object_widths() {
        for (algorithm, left, right, unrelated) in [
            (
                GitHashAlgorithm::Sha1,
                "1234567000000000000000000000000000000000",
                "1234567100000000000000000000000000000000",
                "abcdef0000000000000000000000000000000000",
            ),
            (
                GitHashAlgorithm::Sha256,
                "1234567000000000000000000000000000000000000000000000000000000000",
                "1234567100000000000000000000000000000000000000000000000000000000",
                "abcdef0000000000000000000000000000000000000000000000000000000000",
            ),
        ] {
            let objects = TempDir::new().expect("object dir");
            let left = fake_loose_object(objects.path(), left);
            let right = fake_loose_object(objects.path(), right);
            let unrelated = fake_loose_object(objects.path(), unrelated);
            let store = LooseObjectStore::new(objects.path(), algorithm);
            let ids = [left.clone(), right.clone(), unrelated.clone(), left.clone()];
            let lengths =
                rendered_abbrev_len_for_ids(&store, CoreAbbrevConfigState::Minimum(7), &ids)
                    .expect("per-object abbreviation lengths");
            assert_eq!(
                ids.iter()
                    .map(|id| lengths.width_for(id))
                    .collect::<Vec<_>>(),
                [8, 8, 7, 8]
            );
        }
    }

    #[test]
    fn borrowed_abbrev_widths_keep_row_order_and_duplicates() {
        for (algorithm, left, right, unrelated, expected) in [
            (
                GitHashAlgorithm::Sha1,
                "1234567000000000000000000000000000000000",
                "1234567100000000000000000000000000000000",
                "abcdef0000000000000000000000000000000000",
                [8, 8, 7, 8],
            ),
            (
                GitHashAlgorithm::Sha256,
                "1234567000000000000000000000000000000000000000000000000000000000",
                "1234567100000000000000000000000000000000000000000000000000000000",
                "abcdef0000000000000000000000000000000000000000000000000000000000",
                [8, 8, 7, 8],
            ),
        ] {
            let objects = TempDir::new().expect("object dir");
            let left = fake_loose_object(objects.path(), left);
            let right = fake_loose_object(objects.path(), right);
            let unrelated = fake_loose_object(objects.path(), unrelated);
            let store = LooseObjectStore::new(objects.path(), algorithm);
            let ids = BorrowedObjectIds::from_iter([&left, &right, &unrelated, &left]);
            let widths = rendered_abbrev_widths_for_borrowed_ids(
                &store,
                CoreAbbrevConfigState::Minimum(7),
                &ids,
            )
            .expect("borrowed abbreviation widths");
            assert_eq!(
                (0..ids.len())
                    .map(|index| widths.width_for(index))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn in_memory_abbrev_checks_absent_targets_together() {
        for (algorithm, left, right, expected) in [
            (
                GitHashAlgorithm::Sha1,
                "1234567000000000000000000000000000000000",
                "1234567100000000000000000000000000000000",
                8,
            ),
            (
                GitHashAlgorithm::Sha256,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa0aaaaaaaaaaaaaaaaaaaaaa",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1aaaaaaaaaaaaaaaaaaaaaa",
                42,
            ),
        ] {
            let left_id = ObjectId::from_hex(algorithm, left).expect("left object id");
            let right_id = ObjectId::from_hex(algorithm, right).expect("right object id");
            let lengths =
                RenderedAbbrevLengths::from_ids_minimum(&[left_id.clone(), right_id], 7, algorithm);
            assert_eq!(lengths.width_for(&left_id), expected);
        }
    }

    #[test]
    fn in_memory_abbrev_deduplicates_repeated_targets() {
        let target = ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            "1234567000000000000000000000000000000000",
        )
        .expect("target object id");
        let lengths = RenderedAbbrevLengths::from_ids_minimum(
            &[target.clone(), target.clone()],
            7,
            GitHashAlgorithm::Sha1,
        );
        assert_eq!(lengths.width_for(&target), 7);
    }

    #[test]
    fn revision_abbrev_matches_strtoul_sign_and_width_edges() {
        let ulong_max = if std::mem::size_of::<std::os::raw::c_ulong>() == 4 {
            u32::MAX as u128
        } else {
            u64::MAX as u128
        };
        let ulong_max_text = format!("-{ulong_max}");
        let ulong_overflow_text = format!("-{}", ulong_max + 1);
        let sha1_full = full_abbrev_len(GitHashAlgorithm::Sha1);
        let sha256_full = full_abbrev_len(GitHashAlgorithm::Sha256);

        for (input, expected) in [
            ("", 4),
            ("bogus", 4),
            ("0", 4),
            ("+1", 4),
            ("-1", sha1_full),
            ("-2", sha1_full),
            ("4", 4),
            ("12junk", 12),
            ("184467440737095516160", sha1_full),
        ] {
            assert_eq!(
                parse_revision_abbrev(input, GitHashAlgorithm::Sha1),
                expected,
                "input={input:?}"
            );
        }
        assert_eq!(
            parse_revision_abbrev(&ulong_max_text, GitHashAlgorithm::Sha1),
            4
        );
        assert_eq!(
            parse_revision_abbrev(&ulong_overflow_text, GitHashAlgorithm::Sha1),
            sha1_full
        );
        assert_eq!(
            parse_revision_abbrev("\t\r 12junk", GitHashAlgorithm::Sha256),
            12
        );
        assert_eq!(
            parse_revision_abbrev("999999999999999999999999", GitHashAlgorithm::Sha256),
            sha256_full
        );
    }
}
