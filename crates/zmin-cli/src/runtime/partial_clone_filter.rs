//! Pure partial-clone filter option state.
//!
//! The representation follows Git v2.55.0's
//! `list_objects_filter_options`: a first filter keeps its raw spelling,
//! subsequent filters are retained in order and rendered as a `combine:`
//! filter.  Actions are applied in argv order, so `--no-filter` resets the
//! current choice and a later `--filter` wins.  Transport code can consume
//! this state later without having to reconstruct command-line intent.

const RESERVED_NON_WHITESPACE: &[u8] = b"~`!@#$^&*()[]{}\\;'\",<>?";
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
const SPARSE_OID_PREFIX: &[u8] = b"sparse:oid=";

/// Git v2.55 does not define parser-side resource caps for filter options.
/// These Zmin safety limits bound argv-driven allocation and expansion while
/// preserving ordinary Git filter requests.
pub(crate) const PARTIAL_CLONE_FILTER_MAX_COUNT: usize = 128;
pub(crate) const PARTIAL_CLONE_FILTER_MAX_RAW_LEN: usize = 64 * 1024;
pub(crate) const PARTIAL_CLONE_FILTER_MAX_COMBINED_ENCODED_LEN: usize = 1024 * 1024;

/// The upstream behavior this module intentionally mirrors.
pub(crate) const PARTIAL_CLONE_FILTER_GIT_VERSION: &str = "2.55.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartialCloneFilterValue {
    raw: String,
}

impl PartialCloneFilterValue {
    pub(crate) fn new(raw: String) -> Self {
        Self { raw }
    }

    pub(crate) fn raw(&self) -> &str {
        &self.raw
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PartialCloneFilterValues {
    values: Vec<PartialCloneFilterValue>,
}

impl PartialCloneFilterValues {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, value: PartialCloneFilterValue) {
        self.values.push(value);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    pub(crate) fn as_slice(&self) -> &[PartialCloneFilterValue] {
        &self.values
    }

    pub(crate) fn filter_spec(&self) -> Option<String> {
        match self.values.as_slice() {
            [] => None,
            [value] => Some(value.raw().to_owned()),
            values => {
                let capacity = combined_filter_spec_length(values).unwrap_or(0);
                let mut combined = String::with_capacity(capacity);
                combined.push_str("combine:");
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        combined.push('+');
                    }
                    append_combine_encoded(&mut combined, value.raw());
                }
                Some(combined)
            }
        }
    }
}

fn combined_filter_spec_length(values: &[PartialCloneFilterValue]) -> Option<usize> {
    match values {
        [] => Some(0),
        [value] => Some(value.raw().len()),
        values => {
            let mut length = "combine:".len();
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    length = length.checked_add(1)?;
                }
                length = length.checked_add(encoded_component_length(value.raw())?)?;
            }
            Some(length)
        }
    }
}

fn combined_filter_spec_length_with(
    values: &[PartialCloneFilterValue],
    raw: &str,
) -> Option<usize> {
    let next_count = values.len().checked_add(1)?;
    if next_count == 1 {
        return Some(raw.len());
    }
    let mut length = "combine:".len();
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            length = length.checked_add(1)?;
        }
        length = length.checked_add(encoded_component_length(value.raw())?)?;
    }
    if !values.is_empty() {
        length = length.checked_add(1)?;
    }
    length.checked_add(encoded_component_length(raw)?)
}

fn encoded_component_length(raw: &str) -> Option<usize> {
    raw.as_bytes().iter().try_fold(0usize, |length, byte| {
        length.checked_add(if allow_unencoded(*byte) { 1 } else { 3 })
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PartialCloneFilterSelection {
    Unspecified,
    Auto,
    Explicit(PartialCloneFilterValues),
}

impl Default for PartialCloneFilterSelection {
    fn default() -> Self {
        Self::Unspecified
    }
}

impl PartialCloneFilterSelection {
    pub(crate) fn filter_spec(&self) -> Option<String> {
        match self {
            Self::Unspecified => None,
            Self::Auto => Some(String::from("auto")),
            Self::Explicit(values) => values.filter_spec(),
        }
    }

    pub(crate) fn is_unspecified(&self) -> bool {
        matches!(self, Self::Unspecified)
    }

    pub(crate) fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }

    pub(crate) fn values(&self) -> Option<&PartialCloneFilterValues> {
        match self {
            Self::Explicit(values) => Some(values),
            Self::Unspecified | Self::Auto => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PartialCloneFilterAction {
    Filter(PartialCloneFilterValue),
    NoFilter,
}

impl PartialCloneFilterAction {
    pub(crate) fn filter(raw: String) -> Self {
        Self::Filter(PartialCloneFilterValue::new(raw))
    }

    pub(crate) fn no_filter() -> Self {
        Self::NoFilter
    }
}

/// Ordered command-line events for Git's two partial-clone filter spellings.
/// The schema stores option families separately; dispatch must reconstruct
/// their argv order before applying the state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PartialCloneFilterCliEvent {
    Filter(String),
    NoFilter,
}

pub(crate) fn parse_partial_clone_filter_events(
    raw_args: &[String],
) -> Vec<PartialCloneFilterCliEvent> {
    let mut events = Vec::new();
    let mut index = 1;
    while index < raw_args.len() {
        let argument = &raw_args[index];
        if argument == "--" {
            break;
        }
        if argument == "--no-filter" {
            events.push(PartialCloneFilterCliEvent::NoFilter);
            index += 1;
            continue;
        }
        if argument == "--filter" {
            if let Some(value) = raw_args.get(index + 1) {
                events.push(PartialCloneFilterCliEvent::Filter(value.clone()));
                index += 2;
                continue;
            }
            index += 1;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--filter=") {
            events.push(PartialCloneFilterCliEvent::Filter(value.to_owned()));
        }
        index += 1;
    }
    events
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PartialCloneFilterCommandContext {
    Clone,
    Fetch,
    Transport,
    #[default]
    Other,
}

impl PartialCloneFilterCommandContext {
    pub(crate) fn allows_auto_filter(self) -> bool {
        matches!(self, Self::Clone | Self::Fetch | Self::Transport)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PartialCloneFilterPolicy {
    context: PartialCloneFilterCommandContext,
    allow_auto_filter: bool,
}

impl Default for PartialCloneFilterPolicy {
    fn default() -> Self {
        Self::for_context(PartialCloneFilterCommandContext::Other)
    }
}

impl PartialCloneFilterPolicy {
    pub(crate) fn for_context(context: PartialCloneFilterCommandContext) -> Self {
        Self {
            context,
            allow_auto_filter: context.allows_auto_filter(),
        }
    }

    pub(crate) fn context(self) -> PartialCloneFilterCommandContext {
        self.context
    }

    pub(crate) fn allow_auto_filter(self) -> bool {
        self.allow_auto_filter
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PartialCloneFilterError {
    EmptyFilterSpec,
    EmptyCombineFilterSpec,
    InvalidFilterSpec {
        raw: String,
    },
    InvalidPercentEscape {
        raw: String,
    },
    InvalidUtf8FilterSpec {
        raw: String,
    },
    ReservedCombineCharacter {
        raw: String,
    },
    LiteralControlCharacter {
        raw: String,
    },
    ExpectedTreeDepth {
        raw: String,
    },
    SparsePathFiltersDropped {
        raw: String,
    },
    AutoFilterNotAllowed {
        raw: String,
    },
    AutoCannotCombine {
        raw: String,
    },
    ResourceLimitExceeded {
        limit: PartialCloneFilterResourceLimit,
        actual: usize,
        maximum: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartialCloneFilterResourceLimit {
    FilterCount,
    RawFilterLength,
    CombinedEncodedLength,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PartialCloneFilterState {
    selection: PartialCloneFilterSelection,
    no_filter: bool,
    policy: PartialCloneFilterPolicy,
}

impl PartialCloneFilterState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn for_context(context: PartialCloneFilterCommandContext) -> Self {
        Self::with_policy(PartialCloneFilterPolicy::for_context(context))
    }

    pub(crate) fn with_policy(policy: PartialCloneFilterPolicy) -> Self {
        Self {
            selection: PartialCloneFilterSelection::Unspecified,
            no_filter: false,
            policy,
        }
    }

    pub(crate) fn apply(
        &mut self,
        action: PartialCloneFilterAction,
    ) -> Result<(), PartialCloneFilterError> {
        match action {
            PartialCloneFilterAction::NoFilter => {
                self.selection = PartialCloneFilterSelection::Unspecified;
                self.no_filter = true;
                Ok(())
            }
            PartialCloneFilterAction::Filter(value) => self.apply_filter(value),
        }
    }

    pub(crate) fn apply_actions(
        &mut self,
        actions: impl IntoIterator<Item = PartialCloneFilterAction>,
    ) -> Result<(), PartialCloneFilterError> {
        for action in actions {
            self.apply(action)?;
        }
        Ok(())
    }

    pub(crate) fn apply_cli_events(
        &mut self,
        events: impl IntoIterator<Item = PartialCloneFilterCliEvent>,
    ) -> Result<(), PartialCloneFilterError> {
        for event in events {
            let action = match event {
                PartialCloneFilterCliEvent::Filter(raw) => PartialCloneFilterAction::filter(raw),
                PartialCloneFilterCliEvent::NoFilter => PartialCloneFilterAction::no_filter(),
            };
            self.apply(action)?;
        }
        Ok(())
    }

    pub(crate) fn apply_filter(
        &mut self,
        value: PartialCloneFilterValue,
    ) -> Result<(), PartialCloneFilterError> {
        if self.selection.is_auto() {
            return Err(PartialCloneFilterError::AutoCannotCombine {
                raw: value.raw().to_owned(),
            });
        }
        self.validate_raw_filter_length(value.raw())?;
        let parsed = validate_filter_spec(value.raw(), self.policy.allow_auto_filter)?;
        if matches!(parsed, ValidatedFilterSpec::Auto)
            && matches!(&self.selection, PartialCloneFilterSelection::Explicit(_))
        {
            return Err(PartialCloneFilterError::AutoCannotCombine {
                raw: value.raw().to_owned(),
            });
        }
        let count = match &self.selection {
            PartialCloneFilterSelection::Explicit(values) => {
                values.len().checked_add(1).unwrap_or(usize::MAX)
            }
            PartialCloneFilterSelection::Unspecified | PartialCloneFilterSelection::Auto => 1,
        };
        self.validate_filter_count_and_combined_length(count, value.raw())?;
        match parsed {
            ValidatedFilterSpec::Auto => {
                self.selection = PartialCloneFilterSelection::Auto;
            }
            ValidatedFilterSpec::Other => {
                let selection = std::mem::take(&mut self.selection);
                self.selection = match selection {
                    PartialCloneFilterSelection::Unspecified => {
                        let mut values = PartialCloneFilterValues::new();
                        values.push(value);
                        PartialCloneFilterSelection::Explicit(values)
                    }
                    PartialCloneFilterSelection::Explicit(mut values) => {
                        values.push(value);
                        PartialCloneFilterSelection::Explicit(values)
                    }
                    PartialCloneFilterSelection::Auto => unreachable!("auto handled above"),
                };
            }
        }
        self.no_filter = false;
        Ok(())
    }

    fn validate_raw_filter_length(&self, raw: &str) -> Result<(), PartialCloneFilterError> {
        let actual = raw.len();
        if actual > PARTIAL_CLONE_FILTER_MAX_RAW_LEN {
            return Err(PartialCloneFilterError::ResourceLimitExceeded {
                limit: PartialCloneFilterResourceLimit::RawFilterLength,
                actual,
                maximum: PARTIAL_CLONE_FILTER_MAX_RAW_LEN,
            });
        }
        Ok(())
    }

    fn validate_filter_count_and_combined_length(
        &self,
        count: usize,
        raw: &str,
    ) -> Result<(), PartialCloneFilterError> {
        if count > PARTIAL_CLONE_FILTER_MAX_COUNT {
            return Err(PartialCloneFilterError::ResourceLimitExceeded {
                limit: PartialCloneFilterResourceLimit::FilterCount,
                actual: count,
                maximum: PARTIAL_CLONE_FILTER_MAX_COUNT,
            });
        }

        let combined_length = match &self.selection {
            PartialCloneFilterSelection::Explicit(values) => {
                combined_filter_spec_length_with(values.as_slice(), raw)
            }
            PartialCloneFilterSelection::Unspecified | PartialCloneFilterSelection::Auto => {
                Some(raw.len())
            }
        }
        .unwrap_or(usize::MAX);
        if combined_length > PARTIAL_CLONE_FILTER_MAX_COMBINED_ENCODED_LEN {
            return Err(PartialCloneFilterError::ResourceLimitExceeded {
                limit: PartialCloneFilterResourceLimit::CombinedEncodedLength,
                actual: combined_length,
                maximum: PARTIAL_CLONE_FILTER_MAX_COMBINED_ENCODED_LEN,
            });
        }
        Ok(())
    }

    pub(crate) fn selection(&self) -> &PartialCloneFilterSelection {
        &self.selection
    }

    pub(crate) fn no_filter_requested(&self) -> bool {
        self.no_filter
    }

    pub(crate) fn policy(&self) -> PartialCloneFilterPolicy {
        self.policy
    }

    pub(crate) fn filter_spec(&self) -> Option<String> {
        self.selection.filter_spec()
    }

    /// Returns the filter that may be handed to a transport operation.
    ///
    pub(crate) fn effective_filter_spec(&self) -> Option<String> {
        self.filter_spec()
    }

    pub(crate) fn filter_is_active(&self) -> bool {
        !self.selection.is_unspecified()
    }

    pub(crate) fn inherits_configured_filter(&self) -> bool {
        self.selection.is_unspecified() && !self.no_filter
    }

    pub(crate) fn may_register_partial_clone(&self) -> bool {
        self.filter_is_active() && !self.no_filter
    }
}

fn append_combine_encoded(output: &mut String, raw: &str) {
    let encoded = encode_combine_bytes(raw.as_bytes());
    output.push_str(
        std::str::from_utf8(&encoded).expect("Git filter arguments are valid UTF-8 strings"),
    );
}

fn encode_combine_bytes(raw: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(raw.len());
    for &byte in raw {
        if allow_unencoded(byte) {
            encoded.push(byte);
        } else {
            encoded.push(b'%');
            encoded.push(HEX_DIGITS[(byte >> 4) as usize]);
            encoded.push(HEX_DIGITS[(byte & 0x0f) as usize]);
        }
    }
    encoded
}

fn allow_unencoded(byte: u8) -> bool {
    byte.is_ascii()
        && byte > b' '
        && byte != b'%'
        && byte != b'+'
        && !RESERVED_NON_WHITESPACE.contains(&byte)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValidatedFilterSpec {
    Auto,
    Other,
}

fn validate_filter_spec(
    raw: &str,
    allow_auto_filter: bool,
) -> Result<ValidatedFilterSpec, PartialCloneFilterError> {
    if raw.is_empty() {
        return Err(PartialCloneFilterError::EmptyFilterSpec);
    }
    if raw == "auto" {
        if allow_auto_filter {
            return Ok(ValidatedFilterSpec::Auto);
        }
        return Err(PartialCloneFilterError::AutoFilterNotAllowed {
            raw: raw.to_owned(),
        });
    }
    if raw == "blob:none" {
        return Ok(ValidatedFilterSpec::Other);
    }
    if let Some(limit) = raw.strip_prefix("blob:limit=") {
        if parse_git_unsigned(limit) {
            return Ok(ValidatedFilterSpec::Other);
        }
        return Err(PartialCloneFilterError::InvalidFilterSpec {
            raw: raw.to_owned(),
        });
    }
    if let Some(depth) = raw.strip_prefix("tree:") {
        if parse_git_unsigned(depth) {
            return Ok(ValidatedFilterSpec::Other);
        }
        return Err(PartialCloneFilterError::ExpectedTreeDepth {
            raw: raw.to_owned(),
        });
    }
    if raw.starts_with("sparse:path=") {
        return Err(PartialCloneFilterError::SparsePathFiltersDropped {
            raw: raw.to_owned(),
        });
    }
    if let Some(opaque) = raw.as_bytes().strip_prefix(SPARSE_OID_PREFIX) {
        if opaque.iter().copied().any(is_literal_control_byte) {
            return Err(PartialCloneFilterError::LiteralControlCharacter {
                raw: raw.to_owned(),
            });
        }
        return Ok(ValidatedFilterSpec::Other);
    }
    if let Some(object_type) = raw.strip_prefix("object:type=") {
        if matches!(object_type, "blob" | "tree" | "commit" | "tag") {
            return Ok(ValidatedFilterSpec::Other);
        }
        return Err(PartialCloneFilterError::InvalidFilterSpec {
            raw: raw.to_owned(),
        });
    }
    if let Some(components) = raw.strip_prefix("combine:") {
        return validate_combine_filter(components, allow_auto_filter);
    }
    Err(PartialCloneFilterError::InvalidFilterSpec {
        raw: raw.to_owned(),
    })
}

fn is_literal_control_byte(byte: u8) -> bool {
    byte <= 0x1f || byte == 0x7f
}

fn validate_combine_filter(
    components: &str,
    allow_auto_filter: bool,
) -> Result<ValidatedFilterSpec, PartialCloneFilterError> {
    if components.is_empty() {
        return Err(PartialCloneFilterError::EmptyCombineFilterSpec);
    }
    for component in components.split('+') {
        if component.is_empty() {
            continue;
        }
        if component
            .bytes()
            .any(|byte| !byte.is_ascii() || byte <= b' ' || RESERVED_NON_WHITESPACE.contains(&byte))
        {
            return Err(PartialCloneFilterError::ReservedCombineCharacter {
                raw: component.to_owned(),
            });
        }
        if git_percent_decode_bytes(component.as_bytes()).starts_with(SPARSE_OID_PREFIX) {
            continue;
        }
        let decoded = percent_decode_component(component)?;
        match validate_filter_spec(&decoded, allow_auto_filter)? {
            ValidatedFilterSpec::Auto => {
                return Err(PartialCloneFilterError::AutoCannotCombine { raw: decoded });
            }
            ValidatedFilterSpec::Other => {}
        }
    }
    Ok(ValidatedFilterSpec::Other)
}

fn git_percent_decode_bytes(raw: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' && index + 2 < raw.len() {
            if let (Some(high), Some(low)) = (
                decode_hex_digit(raw[index + 1]),
                decode_hex_digit(raw[index + 2]),
            ) {
                let value = (high << 4) | low;
                if value != 0 {
                    decoded.push(value);
                    index += 3;
                    continue;
                }
            }
        }
        decoded.push(raw[index]);
        index += 1;
    }
    decoded
}

fn percent_decode_component(raw: &str) -> Result<String, PartialCloneFilterError> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(PartialCloneFilterError::InvalidPercentEscape {
                raw: raw.to_owned(),
            });
        }
        let Some(high) = decode_hex_digit(bytes[index + 1]) else {
            return Err(PartialCloneFilterError::InvalidPercentEscape {
                raw: raw.to_owned(),
            });
        };
        let Some(low) = decode_hex_digit(bytes[index + 2]) else {
            return Err(PartialCloneFilterError::InvalidPercentEscape {
                raw: raw.to_owned(),
            });
        };
        decoded.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(decoded).map_err(|_| PartialCloneFilterError::InvalidUtf8FilterSpec {
        raw: raw.to_owned(),
    })
}

fn parse_git_unsigned(raw: &str) -> bool {
    let raw = raw.trim_start_matches(|character: char| character.is_ascii_whitespace());
    if raw.is_empty() || raw.bytes().any(|byte| byte == b'-') {
        return false;
    }
    let (number, multiplier) = match raw.as_bytes().last().copied() {
        Some(b'k' | b'K') => (&raw[..raw.len() - 1], 1024_u128),
        Some(b'm' | b'M') => (&raw[..raw.len() - 1], 1024_u128 * 1024),
        Some(b'g' | b'G') => (&raw[..raw.len() - 1], 1024_u128 * 1024 * 1024),
        _ => (raw, 1),
    };
    if number.is_empty() {
        return false;
    }
    let number = number.strip_prefix('+').unwrap_or(number);
    let (radix, digits) = if let Some(rest) = number
        .strip_prefix("0x")
        .or_else(|| number.strip_prefix("0X"))
    {
        (16, rest)
    } else if number.len() > 1 && number.starts_with('0') {
        (8, &number[1..])
    } else {
        (10, number)
    };
    let value = if digits.is_empty() {
        radix == 8 && number == "0"
    } else {
        let valid_digit = |byte: u8| match radix {
            8 => (b'0'..=b'7').contains(&byte),
            10 => byte.is_ascii_digit(),
            16 => byte.is_ascii_hexdigit(),
            _ => false,
        };
        if !digits.bytes().all(valid_digit) {
            return false;
        }
        u128::from_str_radix(digits, radix).is_ok()
    };
    if !value {
        return false;
    }
    let parsed = if digits.is_empty() {
        0
    } else {
        match u128::from_str_radix(digits, radix) {
            Ok(value) => value,
            Err(_) => return false,
        }
    };
    parsed
        .checked_mul(multiplier)
        .is_some_and(|value| value <= u128::from(u64::MAX))
}

fn decode_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_filters(filters: &[&str]) -> PartialCloneFilterState {
        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
        for filter in filters {
            state
                .apply(PartialCloneFilterAction::filter((*filter).to_owned()))
                .expect("filter should be accepted");
        }
        state
    }

    #[test]
    fn one_filter_keeps_raw_spelling() {
        let state = state_with_filters(&["blob:limit=1k"]);
        assert_eq!(state.filter_spec().as_deref(), Some("blob:limit=1k"));
    }

    #[test]
    fn many_filters_combine_in_original_order() {
        let state = state_with_filters(&["blob:none", "tree:0", "object:type=blob"]);
        assert_eq!(
            state.filter_spec().as_deref(),
            Some("combine:blob:none+tree:0+object:type=blob")
        );
    }

    #[test]
    fn combine_encoding_matches_git_v255_allowlist() {
        let state = state_with_filters(&["sparse:oid=a b+%~`!@#$^&*()[]{}\\;'\",<>?:/="]);
        let state = {
            let mut state = state;
            state
                .apply(PartialCloneFilterAction::filter("blob:none".to_owned()))
                .expect("second filter should be accepted");
            state
        };
        assert_eq!(
            state.filter_spec().as_deref(),
            Some(
                "combine:sparse:oid=a%20b%2b%25%7e%60%21%40%23%24%5e%26%2a%28%29%5b%5d%7b%7d%5c%3b%27%22%2c%3c%3e%3f:/=+blob:none"
            )
        );
    }

    #[test]
    fn nested_combine_is_encoded_as_one_raw_component() {
        let state = state_with_filters(&["combine:blob:none+tree:0", "object:type=blob"]);
        assert_eq!(
            state.filter_spec().as_deref(),
            Some("combine:combine:blob:none%2btree:0+object:type=blob")
        );
    }

    #[test]
    fn non_ascii_filter_spelling_is_percent_encoded_when_combined() {
        let state = state_with_filters(&["sparse:oid=café", "blob:none"]);
        assert_eq!(
            state.filter_spec().as_deref(),
            Some("combine:sparse:oid=caf%c3%a9+blob:none")
        );
    }

    #[test]
    fn raw_utf8_in_combine_subfilter_is_rejected() {
        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
        assert_eq!(
            state.apply(PartialCloneFilterAction::filter(
                "combine:sparse:oid=café+blob:none".to_owned(),
            )),
            Err(PartialCloneFilterError::ReservedCombineCharacter {
                raw: "sparse:oid=café".to_owned(),
            })
        );
        assert!(state.selection().is_unspecified());
    }

    #[test]
    fn sparse_oid_combine_preserves_git_opaque_percent_sequences() {
        for filter in [
            "combine:sparse:oid=%GG+blob:none",
            "combine:sparse:oid=%2+blob:none",
            "combine:sparse:oid=%ff+blob:none",
            "combine:sparse:oid=%00+blob:none",
            "combine:sparse%3aoid=%GG+blob:none",
        ] {
            let state = state_with_filters(&[filter]);
            assert_eq!(state.filter_spec().as_deref(), Some(filter), "{filter}");
        }
    }

    #[test]
    fn sparse_oid_rejects_literal_controls_but_accepts_encoded_opaque_bytes() {
        for filter in [
            "sparse:oid=literal\0",
            "sparse:oid=literal\n",
            "sparse:oid=literal\r",
            "sparse:oid=literal\u{1f}",
            "sparse:oid=literal\u{7f}",
        ] {
            let mut state =
                PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
            assert_eq!(
                state.apply(PartialCloneFilterAction::filter(filter.to_owned())),
                Err(PartialCloneFilterError::LiteralControlCharacter {
                    raw: filter.to_owned(),
                }),
                "{filter:?}"
            );
            assert!(state.selection().is_unspecified(), "{filter:?}");
        }

        for filter in [
            "sparse:oid=%00",
            "sparse:oid=%0a",
            "sparse:oid=%0d",
            "sparse:oid=%1f",
            "sparse:oid=%7f",
        ] {
            let state = state_with_filters(&[filter]);
            assert_eq!(state.filter_spec().as_deref(), Some(filter), "{filter}");
        }
    }

    #[test]
    fn encoded_sparse_oid_controls_are_encoded_again_when_combined() {
        let state = state_with_filters(&["sparse:oid=%00", "blob:none"]);
        assert_eq!(
            state.filter_spec().as_deref(),
            Some("combine:sparse:oid=%2500+blob:none")
        );
    }

    #[test]
    fn structured_combine_filters_reject_opaque_percent_sequences() {
        let probes = [
            (
                "combine:blob:none%GG",
                PartialCloneFilterError::InvalidPercentEscape {
                    raw: "blob:none%GG".to_owned(),
                },
            ),
            (
                "combine:blob:none%2",
                PartialCloneFilterError::InvalidPercentEscape {
                    raw: "blob:none%2".to_owned(),
                },
            ),
            (
                "combine:tree:%ff",
                PartialCloneFilterError::InvalidUtf8FilterSpec {
                    raw: "tree:%ff".to_owned(),
                },
            ),
            (
                "combine:object:type=%GG",
                PartialCloneFilterError::InvalidPercentEscape {
                    raw: "object:type=%GG".to_owned(),
                },
            ),
        ];

        for (filter, expected) in probes {
            let mut state =
                PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
            assert_eq!(
                state.apply(PartialCloneFilterAction::filter(filter.to_owned())),
                Err(expected),
                "{filter}"
            );
            assert!(state.selection().is_unspecified(), "{filter}");
        }
    }

    #[test]
    fn empty_values_and_components_are_not_dropped() {
        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
        assert_eq!(
            state.apply(PartialCloneFilterAction::filter(String::new())),
            Err(PartialCloneFilterError::EmptyFilterSpec)
        );

        let state = state_with_filters(&["combine:+blob:none+"]);
        assert_eq!(state.filter_spec().as_deref(), Some("combine:+blob:none+"));

        let state = state_with_filters(&["blob:none", "tree:0"]);
        assert_eq!(
            state.filter_spec().as_deref(),
            Some("combine:blob:none+tree:0")
        );

        let values = PartialCloneFilterValues {
            values: vec![
                PartialCloneFilterValue::new(String::new()),
                PartialCloneFilterValue::new(String::from("blob:none")),
                PartialCloneFilterValue::new(String::new()),
            ],
        };
        assert_eq!(values.filter_spec().as_deref(), Some("combine:+blob:none+"));
    }

    #[test]
    fn valid_stock_filter_specs_keep_raw_spelling() {
        for filter in [
            "blob:none",
            "blob:limit=1k",
            "tree:0x10",
            "sparse:oid=",
            "object:type=tag",
            "combine:blob%3anone+tree%3a0",
            "combine:+blob:none+",
            "combine:++",
        ] {
            let state = state_with_filters(&[filter]);
            assert_eq!(state.filter_spec().as_deref(), Some(filter), "{filter}");
        }
    }

    #[test]
    fn invalid_stock_filter_specs_are_rejected() {
        let probes = [
            ("", PartialCloneFilterError::EmptyFilterSpec),
            (
                "bad",
                PartialCloneFilterError::InvalidFilterSpec {
                    raw: String::from("bad"),
                },
            ),
            (
                "blob:limit=abc",
                PartialCloneFilterError::InvalidFilterSpec {
                    raw: String::from("blob:limit=abc"),
                },
            ),
            (
                "tree:",
                PartialCloneFilterError::ExpectedTreeDepth {
                    raw: String::from("tree:"),
                },
            ),
            (
                "tree:abc",
                PartialCloneFilterError::ExpectedTreeDepth {
                    raw: String::from("tree:abc"),
                },
            ),
            (
                "sparse:path=patterns",
                PartialCloneFilterError::SparsePathFiltersDropped {
                    raw: String::from("sparse:path=patterns"),
                },
            ),
            (
                "object:type=bad",
                PartialCloneFilterError::InvalidFilterSpec {
                    raw: String::from("object:type=bad"),
                },
            ),
            ("combine:", PartialCloneFilterError::EmptyCombineFilterSpec),
            (
                "combine:blob:none%",
                PartialCloneFilterError::InvalidPercentEscape {
                    raw: String::from("blob:none%"),
                },
            ),
            (
                "combine:blob:none%2",
                PartialCloneFilterError::InvalidPercentEscape {
                    raw: String::from("blob:none%2"),
                },
            ),
            (
                "combine:blob:none%2g",
                PartialCloneFilterError::InvalidPercentEscape {
                    raw: String::from("blob:none%2g"),
                },
            ),
            (
                "combine:blob:none~",
                PartialCloneFilterError::ReservedCombineCharacter {
                    raw: String::from("blob:none~"),
                },
            ),
            (
                "combine:blob:none%2b tree:0",
                PartialCloneFilterError::ReservedCombineCharacter {
                    raw: String::from("blob:none%2b tree:0"),
                },
            ),
            (
                "combine:blob:none%2Btree:0",
                PartialCloneFilterError::InvalidFilterSpec {
                    raw: String::from("blob:none+tree:0"),
                },
            ),
        ];

        for (filter, expected) in probes {
            let mut state =
                PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
            assert_eq!(
                state.apply(PartialCloneFilterAction::filter(filter.to_owned())),
                Err(expected),
                "{filter}"
            );
            assert!(state.selection().is_unspecified(), "{filter}");
        }
    }

    #[test]
    fn no_filter_resets_choice_and_later_filter_wins() {
        let mut state = state_with_filters(&["blob:none", "tree:0"]);
        state
            .apply(PartialCloneFilterAction::no_filter())
            .expect("no-filter should always apply");
        assert!(state.selection().is_unspecified());
        assert!(state.no_filter_requested());
        assert!(!state.filter_is_active());
        assert!(!state.inherits_configured_filter());

        state
            .apply(PartialCloneFilterAction::filter("tree:1".to_owned()))
            .expect("an explicit filter after no-filter should be retained");
        assert_eq!(state.filter_spec().as_deref(), Some("tree:1"));
        assert!(!state.no_filter_requested());
        assert!(state.filter_is_active());
        assert!(state.may_register_partial_clone());
        assert_eq!(state.effective_filter_spec().as_deref(), Some("tree:1"));
    }

    #[test]
    fn ordered_actions_are_last_option_wins_and_recombine_after_reset() {
        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
        state
            .apply_actions([
                PartialCloneFilterAction::filter("blob:none".to_owned()),
                PartialCloneFilterAction::no_filter(),
                PartialCloneFilterAction::filter("tree:0".to_owned()),
                PartialCloneFilterAction::filter("object:type=blob".to_owned()),
            ])
            .expect("ordered filter actions should apply");
        assert_eq!(
            state.filter_spec().as_deref(),
            Some("combine:tree:0+object:type=blob")
        );
        assert!(!state.no_filter_requested());
        assert!(state.may_register_partial_clone());

        let events = parse_partial_clone_filter_events(&[
            String::from("fetch"),
            String::from("--filter=blob:none"),
            String::from("--no-filter"),
            String::from("--filter"),
            String::from("tree:1"),
            String::from("--filter=object:type=tag"),
        ]);
        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Fetch);
        state
            .apply_cli_events(events)
            .expect("parsed ordered events should apply");
        assert_eq!(
            state.filter_spec().as_deref(),
            Some("combine:tree:1+object:type=tag")
        );
        assert!(!state.no_filter_requested());
    }

    #[test]
    fn direct_no_filter_probes_are_last_option_wins() {
        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
        state
            .apply_actions([
                PartialCloneFilterAction::filter("blob:none".to_owned()),
                PartialCloneFilterAction::no_filter(),
            ])
            .expect("filter followed by no-filter should apply");
        assert_eq!(state.filter_spec(), None);
        assert!(state.no_filter_requested());
        assert!(!state.may_register_partial_clone());

        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
        state
            .apply_actions([
                PartialCloneFilterAction::no_filter(),
                PartialCloneFilterAction::filter("blob:none".to_owned()),
            ])
            .expect("no-filter followed by filter should apply");
        assert_eq!(state.filter_spec().as_deref(), Some("blob:none"));
        assert!(!state.no_filter_requested());
        assert!(state.may_register_partial_clone());
    }

    #[test]
    fn long_alternating_filter_and_no_filter_sequence_keeps_only_final_run() {
        let mut actions = Vec::new();
        for depth in 0..64 {
            actions.push(PartialCloneFilterAction::filter(format!("tree:{depth}")));
            actions.push(PartialCloneFilterAction::no_filter());
        }
        actions.push(PartialCloneFilterAction::filter("blob:none".to_owned()));
        actions.push(PartialCloneFilterAction::filter("tree:0".to_owned()));

        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
        state
            .apply_actions(actions)
            .expect("alternating actions should apply");
        assert_eq!(
            state.filter_spec().as_deref(),
            Some("combine:blob:none+tree:0")
        );
        assert!(!state.no_filter_requested());
    }

    #[test]
    fn filter_count_bound_is_fatal_and_does_not_mutate_state() {
        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
        state
            .apply_actions(
                (0..PARTIAL_CLONE_FILTER_MAX_COUNT)
                    .map(|_| PartialCloneFilterAction::filter("tree:0".to_owned())),
            )
            .expect("the configured count bound should be inclusive");
        let before = state.filter_spec();
        assert_eq!(
            state.apply(PartialCloneFilterAction::filter("tree:1".to_owned())),
            Err(PartialCloneFilterError::ResourceLimitExceeded {
                limit: PartialCloneFilterResourceLimit::FilterCount,
                actual: PARTIAL_CLONE_FILTER_MAX_COUNT + 1,
                maximum: PARTIAL_CLONE_FILTER_MAX_COUNT,
            })
        );
        assert_eq!(state.filter_spec(), before);
    }

    #[test]
    fn raw_filter_length_bound_is_fatal_before_parser_storage() {
        let raw = "x".repeat(PARTIAL_CLONE_FILTER_MAX_RAW_LEN + 1);
        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
        assert_eq!(
            state.apply(PartialCloneFilterAction::filter(raw.clone())),
            Err(PartialCloneFilterError::ResourceLimitExceeded {
                limit: PartialCloneFilterResourceLimit::RawFilterLength,
                actual: raw.len(),
                maximum: PARTIAL_CLONE_FILTER_MAX_RAW_LEN,
            })
        );
        assert!(state.selection().is_unspecified());
    }

    #[test]
    fn combined_encoded_length_bound_is_fatal_and_preserves_prior_values() {
        let raw = format!(
            "sparse:oid={}",
            "!".repeat(PARTIAL_CLONE_FILTER_MAX_RAW_LEN - SPARSE_OID_PREFIX.len())
        );
        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
        for _ in 0..5 {
            state
                .apply(PartialCloneFilterAction::filter(raw.clone()))
                .expect("five bounded opaque filters should fit");
        }
        let before = state.filter_spec();
        let error = state
            .apply(PartialCloneFilterAction::filter(raw.clone()))
            .expect_err("the combined encoded safety cap should be fatal");
        assert!(matches!(
            error,
            PartialCloneFilterError::ResourceLimitExceeded {
                limit: PartialCloneFilterResourceLimit::CombinedEncodedLength,
                actual,
                maximum: PARTIAL_CLONE_FILTER_MAX_COMBINED_ENCODED_LEN,
            } if actual > PARTIAL_CLONE_FILTER_MAX_COMBINED_ENCODED_LEN
        ));
        assert_eq!(state.filter_spec(), before);
    }

    #[test]
    fn no_filter_without_explicit_option_allows_default_inheritance() {
        let state = PartialCloneFilterState::new();
        assert!(state.inherits_configured_filter());
        assert!(!state.may_register_partial_clone());
    }

    #[test]
    fn auto_is_a_named_selection_and_never_blob_none() {
        let state = state_with_filters(&["auto"]);
        assert!(state.selection().is_auto());
        assert_eq!(state.filter_spec().as_deref(), Some("auto"));
        assert_ne!(state.filter_spec().as_deref(), Some("blob:none"));
        assert!(state.filter_is_active());
    }

    #[test]
    fn auto_cannot_be_combined_or_nested() {
        let mut state = state_with_filters(&["auto"]);
        assert_eq!(
            state.apply(PartialCloneFilterAction::filter("tree:0".to_owned())),
            Err(PartialCloneFilterError::AutoCannotCombine {
                raw: "tree:0".to_owned()
            })
        );

        let mut state = state_with_filters(&["blob:none"]);
        assert_eq!(
            state.apply(PartialCloneFilterAction::filter("auto".to_owned())),
            Err(PartialCloneFilterError::AutoCannotCombine {
                raw: "auto".to_owned()
            })
        );

        let mut state =
            PartialCloneFilterState::for_context(PartialCloneFilterCommandContext::Clone);
        assert_eq!(
            state.apply(PartialCloneFilterAction::filter(
                "combine:blob:none+auto".to_owned()
            )),
            Err(PartialCloneFilterError::AutoCannotCombine {
                raw: "auto".to_owned()
            })
        );
    }

    #[test]
    fn auto_is_allowed_only_in_named_command_contexts() {
        let contexts = [
            (PartialCloneFilterCommandContext::Clone, true),
            (PartialCloneFilterCommandContext::Fetch, true),
            (PartialCloneFilterCommandContext::Transport, true),
            (PartialCloneFilterCommandContext::Other, false),
        ];
        for (context, allowed) in contexts {
            let mut state = PartialCloneFilterState::for_context(context);
            assert_eq!(state.policy().context(), context);
            assert_eq!(state.policy().allow_auto_filter(), allowed);
            let result = state.apply(PartialCloneFilterAction::filter("auto".to_owned()));
            if allowed {
                assert_eq!(result, Ok(()));
                assert!(state.selection().is_auto());
                assert_eq!(state.filter_spec().as_deref(), Some("auto"));
            } else {
                assert_eq!(
                    result,
                    Err(PartialCloneFilterError::AutoFilterNotAllowed {
                        raw: "auto".to_owned()
                    })
                );
                assert!(state.selection().is_unspecified());
                assert_ne!(state.filter_spec().as_deref(), Some("blob:none"));
            }
        }
    }

    #[test]
    fn percent_encoding_property_covers_every_byte() {
        for byte in 0..=u8::MAX {
            let output = encode_combine_bytes(&[byte]);
            let mut expected = Vec::new();
            if allow_unencoded(byte) {
                expected.push(byte);
            } else {
                expected.push(b'%');
                expected.push(HEX_DIGITS[(byte >> 4) as usize]);
                expected.push(HEX_DIGITS[(byte & 0x0f) as usize]);
            }
            assert_eq!(output, expected, "encoded byte {byte:#04x}");
        }
    }
}
