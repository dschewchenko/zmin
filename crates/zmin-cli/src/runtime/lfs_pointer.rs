//! Strict Git LFS v1 pointer parsing and serialization.
//!
//! The wire format follows the Git LFS pointer specification:
//! <https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md>.
//! Extension entries follow the format documented at:
//! <https://github.com/git-lfs/git-lfs/blob/main/docs/extensions.md>.
//!
//! `parse_current` models the current Git LFS parser boundary: it accepts a
//! pointer without a final newline, does not require field order, and keeps
//! unknown fields. `parse_strict` and `check_canonical` are the explicit
//! canonical-format boundary. Deprecated/pre-release version aliases are not
//! accepted by either API.

use std::fmt;
use std::io::{self, Write};

pub(crate) const LFS_POINTER_VERSION: &str = "https://git-lfs.github.com/spec/v1";
pub(crate) const LFS_POINTER_MAX_BYTES: usize = 1024;
pub(crate) const LFS_POINTER_MAX_SIZE: u64 = i64::MAX as u64;

const SHA256_HEX_BYTES: usize = 64;
const SHA256_BYTES: usize = 32;
const SHA256_PREFIX: &str = "sha256:";

/// A SHA-256 Git LFS object id in its canonical lower-case hexadecimal form.
///
/// Both representations are cached: callers can obtain the binary digest by
/// value and serializers can borrow the validated ASCII form without another
/// conversion allocation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LfsOid {
    bytes: [u8; SHA256_BYTES],
    hex: [u8; SHA256_HEX_BYTES],
}

impl LfsOid {
    pub(crate) fn parse(value: &str) -> Result<Self, LfsPointerError> {
        let Some(hex) = value.strip_prefix(SHA256_PREFIX) else {
            return Err(LfsPointerError::InvalidOid);
        };
        Self::from_hex(hex)
    }

    pub(crate) fn from_hex(hex: &str) -> Result<Self, LfsPointerError> {
        if hex.len() != SHA256_HEX_BYTES || !hex.bytes().all(is_lower_hex) {
            return Err(LfsPointerError::InvalidOid);
        }

        let mut bytes = [0; SHA256_BYTES];
        for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
            bytes[index] = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
        }
        let mut hex_bytes = [0; SHA256_HEX_BYTES];
        hex_bytes.copy_from_slice(hex.as_bytes());
        Ok(Self {
            bytes,
            hex: hex_bytes,
        })
    }

    /// Returns the binary digest by value without reparsing the hexadecimal id.
    pub(crate) fn bytes(&self) -> [u8; SHA256_BYTES] {
        self.bytes
    }

    /// Returns the validated lower-case hexadecimal digest without allocation.
    pub(crate) fn hex(&self) -> &str {
        // `hex` is constructed only from ASCII hexadecimal bytes.
        std::str::from_utf8(&self.hex).expect("validated SHA-256 hex is UTF-8")
    }

    pub(crate) fn value(&self) -> String {
        let mut value = String::with_capacity(SHA256_PREFIX.len() + SHA256_HEX_BYTES);
        value.push_str(SHA256_PREFIX);
        value.push_str(self.hex());
        value
    }
}

/// An LFS pointer extension entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsPointerExtension {
    key: String,
    priority: u32,
    name: String,
    oid: LfsOid,
}

impl LfsPointerExtension {
    pub(crate) fn new(key: String, oid: LfsOid) -> Result<Self, LfsPointerError> {
        let (priority, name) = parse_extension_key(&key)?;
        let name = name.to_owned();
        Ok(Self {
            key,
            priority,
            name,
            oid,
        })
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn priority(&self) -> u32 {
        self.priority
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn oid(&self) -> &LfsOid {
        &self.oid
    }
}

/// An unknown current Git LFS pointer field retained for round-trip support.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsPointerExtra {
    key: String,
    value: String,
}

impl LfsPointerExtra {
    pub(crate) fn new(key: String, value: String) -> Result<Self, LfsPointerError> {
        validate_extra_key(&key)?;
        validate_extra_value(&value, true)?;
        Ok(Self { key, value })
    }

    fn from_current(key: String, value: String) -> Result<Self, LfsPointerError> {
        validate_extra_key(&key)?;
        validate_extra_value(&value, false)?;
        Ok(Self { key, value })
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn value(&self) -> &str {
        &self.value
    }
}

/// A Git LFS v1 pointer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsPointer {
    oid: LfsOid,
    size: u64,
    extensions: Vec<LfsPointerExtension>,
    extras: Vec<LfsPointerExtra>,
}

enum CanonicalField<'a> {
    Extension(&'a LfsPointerExtension),
    Extra(&'a LfsPointerExtra),
    Oid(&'a LfsOid),
    Size(u64),
}

impl CanonicalField<'_> {
    fn key(&self) -> &str {
        match self {
            Self::Extension(extension) => extension.key(),
            Self::Extra(extra) => extra.key(),
            Self::Oid(_) => "oid",
            Self::Size(_) => "size",
        }
    }
}

impl LfsPointer {
    /// Creates a canonical pointer without unknown fields.
    pub(crate) fn new(
        oid: LfsOid,
        size: u64,
        extensions: Vec<LfsPointerExtension>,
    ) -> Result<Self, LfsPointerError> {
        Self::new_with_extras(oid, size, extensions, Vec::new())
    }

    pub(crate) fn new_with_extras(
        oid: LfsOid,
        size: u64,
        extensions: Vec<LfsPointerExtension>,
        extras: Vec<LfsPointerExtra>,
    ) -> Result<Self, LfsPointerError> {
        let pointer = Self {
            oid,
            size: checked_size(size)?,
            extensions,
            extras,
        };
        pointer.check_canonical()?;
        Ok(pointer)
    }

    /// Parses a pointer using current Git LFS detection rules.
    ///
    /// Empty input is not an LFS pointer; the filter layer must pass it
    /// through as an empty file. This parser accepts missing final LF and
    /// arbitrary field order, while preserving unknown fields.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, LfsPointerError> {
        Self::parse_current(bytes)
    }

    pub(crate) fn parse_current(bytes: &[u8]) -> Result<Self, LfsPointerError> {
        if bytes.len() >= LFS_POINTER_MAX_BYTES {
            return Err(LfsPointerError::TooLarge);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| LfsPointerError::InvalidUtf8)?;
        validate_text_controls(text)?;
        let content = text.strip_suffix('\n').unwrap_or(text);
        if content.is_empty() {
            return Err(LfsPointerError::MissingRequiredKey);
        }

        let mut version = None;
        let mut oid = None;
        let mut size = None;
        let mut extensions = Vec::new();
        let mut priorities = Vec::new();
        let mut extras: Vec<LfsPointerExtra> = Vec::new();

        for line in content.split('\n') {
            let (key, value) = split_line(line)?;
            if key == "version" {
                if version.is_some() {
                    return Err(LfsPointerError::DuplicateKey);
                }
                if value != LFS_POINTER_VERSION {
                    return Err(LfsPointerError::UnsupportedVersion);
                }
                version = Some(());
            } else if key == "oid" {
                if oid.is_some() {
                    return Err(LfsPointerError::DuplicateKey);
                }
                oid = Some(LfsOid::parse(value)?);
            } else if key == "size" {
                if size.is_some() {
                    return Err(LfsPointerError::DuplicateKey);
                }
                size = Some(parse_size_current(value)?);
            } else if key.starts_with("ext-") {
                let extension = LfsPointerExtension::new(key.to_owned(), LfsOid::parse(value)?)?;
                if priorities.contains(&extension.priority) {
                    return Err(LfsPointerError::DuplicateExtensionPriority);
                }
                priorities.push(extension.priority);
                extensions.push(extension);
            } else {
                if extras.iter().any(|extra| extra.key() == key) {
                    return Err(LfsPointerError::DuplicateKey);
                }
                extras.push(LfsPointerExtra::from_current(
                    key.to_owned(),
                    value.to_owned(),
                )?);
            }
        }

        let Some(()) = version else {
            return Err(LfsPointerError::MissingRequiredKey);
        };
        let Some(oid) = oid else {
            return Err(LfsPointerError::MissingRequiredKey);
        };
        let Some(size) = size else {
            return Err(LfsPointerError::MissingRequiredKey);
        };
        Ok(Self {
            oid,
            size,
            extensions,
            extras,
        })
    }

    /// Parses only the exact canonical v1 encoding.
    pub(crate) fn parse_strict(bytes: &[u8]) -> Result<Self, LfsPointerError> {
        let pointer = Self::parse_current(bytes)?;
        validate_strict_encoding(bytes)?;
        pointer.check_canonical()?;
        Ok(pointer)
    }

    /// Returns whether the bytes are a current v1 pointer, excluding empty
    /// files, which are handled as filter-layer passthrough.
    pub(crate) fn is_current_pointer(bytes: &[u8]) -> bool {
        Self::parse_current(bytes).is_ok()
    }

    /// Checks the in-memory canonical constraints without serializing.
    pub(crate) fn check_canonical(&self) -> Result<(), LfsPointerError> {
        checked_size(self.size)?;
        validate_extensions(&self.extensions)?;
        validate_extras(&self.extras)?;
        validate_canonical_fields(self)?;
        if self.serialized_len()? >= LFS_POINTER_MAX_BYTES {
            return Err(LfsPointerError::TooLarge);
        }
        Ok(())
    }

    pub(crate) fn oid(&self) -> &LfsOid {
        &self.oid
    }

    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    pub(crate) fn extensions(&self) -> &[LfsPointerExtension] {
        &self.extensions
    }

    pub(crate) fn extras(&self) -> &[LfsPointerExtra] {
        &self.extras
    }

    fn canonical_fields(&self) -> Vec<CanonicalField<'_>> {
        let mut fields = Vec::new();
        fields.extend(self.extensions.iter().map(CanonicalField::Extension));
        fields.extend(self.extras.iter().map(CanonicalField::Extra));
        fields.push(CanonicalField::Oid(&self.oid));
        fields.push(CanonicalField::Size(self.size));
        fields
    }

    /// Serializes this value in the canonical v1 encoding.
    pub(crate) fn serialize_canonical(&self) -> Result<Vec<u8>, LfsPointerError> {
        self.to_bytes()
    }

    /// Returns the size of the canonical encoding.
    pub(crate) fn serialized_len(&self) -> Result<usize, LfsPointerError> {
        checked_size(self.size)?;
        let mut length = line_len("version", LFS_POINTER_VERSION)?;
        for extra in &self.extras {
            length = checked_add(length, line_len(extra.key(), extra.value())?)?;
        }
        for extension in &self.extensions {
            length = checked_add(length, extension_line_len(extension)?)?;
        }
        length = checked_add(length, oid_line_len("oid")?)?;
        checked_add(length, size_line_len(self.size)?)
    }

    /// Serializes canonically without constructing the whole pointer first.
    pub(crate) fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), LfsPointerError> {
        let length = self.serialized_len()?;
        if length >= LFS_POINTER_MAX_BYTES {
            return Err(LfsPointerError::TooLarge);
        }
        validate_serializable_values(self)?;

        writer
            .write_all(b"version ")
            .and_then(|_| writer.write_all(LFS_POINTER_VERSION.as_bytes()))
            .and_then(|_| writer.write_all(b"\n"))
            .map_err(LfsPointerError::io)?;

        let mut fields = self.canonical_fields();
        fields.sort_unstable_by(compare_canonical_fields);
        for field in fields {
            write_field(writer, field)?;
        }
        Ok(())
    }

    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, LfsPointerError> {
        let length = self.serialized_len()?;
        if length >= LFS_POINTER_MAX_BYTES {
            return Err(LfsPointerError::TooLarge);
        }
        let mut bytes = Vec::with_capacity(length);
        self.write_to(&mut bytes)?;
        debug_assert_eq!(bytes.len(), length);
        Ok(bytes)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LfsPointerError {
    InvalidUtf8,
    InvalidControl,
    InvalidLine,
    MissingFinalNewline,
    UnsupportedVersion,
    MissingRequiredKey,
    DuplicateKey,
    DuplicateExtensionPriority,
    NonCanonicalOrder,
    NonCanonicalEncoding,
    InvalidKey,
    InvalidOid,
    InvalidExtensionPriority,
    InvalidSize,
    TooLarge,
    Io(io::ErrorKind),
}

impl LfsPointerError {
    fn io(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

impl fmt::Display for LfsPointerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidUtf8 => "LFS pointer is not UTF-8",
            Self::InvalidControl => "LFS pointer contains a control character",
            Self::InvalidLine => "LFS pointer line is not a canonical key/value line",
            Self::MissingFinalNewline => "LFS pointer is missing its final newline",
            Self::UnsupportedVersion => "unsupported LFS pointer version",
            Self::MissingRequiredKey => "LFS pointer is missing version, oid, or size",
            Self::DuplicateKey => "LFS pointer contains a duplicate key",
            Self::DuplicateExtensionPriority => {
                "LFS pointer contains duplicate extension priorities"
            }
            Self::NonCanonicalOrder => "LFS pointer keys are not in canonical order",
            Self::NonCanonicalEncoding => "LFS pointer is not canonically encoded",
            Self::InvalidKey => "LFS pointer key is invalid",
            Self::InvalidOid => "LFS pointer OID is not a lower-case SHA-256 OID",
            Self::InvalidExtensionPriority => "LFS pointer extension priority is invalid",
            Self::InvalidSize => "LFS pointer size is not a canonical supported integer",
            Self::TooLarge => "LFS pointer is 1024 bytes or larger",
            Self::Io(_) => "could not write LFS pointer",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LfsPointerError {}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => unreachable!("validated lower-case hexadecimal byte"),
    }
}

fn compare_canonical_fields(
    left: &CanonicalField<'_>,
    right: &CanonicalField<'_>,
) -> std::cmp::Ordering {
    match (left, right) {
        (CanonicalField::Extension(left), CanonicalField::Extension(right)) => {
            compare_extension_keys(left, right)
        }
        (CanonicalField::Extension(left), _) => {
            let left_key = canonical_extension_key(left);
            left_key.as_str().cmp(right.key())
        }
        (_, CanonicalField::Extension(right)) => {
            let right_key = canonical_extension_key(right);
            left.key().cmp(right_key.as_str())
        }
        _ => left.key().cmp(right.key()),
    }
}

fn compare_extension_keys(
    left: &LfsPointerExtension,
    right: &LfsPointerExtension,
) -> std::cmp::Ordering {
    let left_key = canonical_extension_key(left);
    let right_key = canonical_extension_key(right);
    left_key.as_str().cmp(right_key.as_str())
}

fn canonical_extension_key(extension: &LfsPointerExtension) -> String {
    format!("ext-{}-{}", extension.priority(), extension.name())
}

fn write_field<W: Write>(writer: &mut W, field: CanonicalField<'_>) -> Result<(), LfsPointerError> {
    match field {
        CanonicalField::Extension(extension) => {
            writer.write_all(b"ext-").map_err(LfsPointerError::io)?;
            write_u32(writer, extension.priority())?;
            writer.write_all(b"-").map_err(LfsPointerError::io)?;
            writer
                .write_all(extension.name().as_bytes())
                .and_then(|_| writer.write_all(b" "))
                .and_then(|_| writer.write_all(SHA256_PREFIX.as_bytes()))
                .and_then(|_| writer.write_all(extension.oid().hex().as_bytes()))
                .and_then(|_| writer.write_all(b"\n"))
                .map_err(LfsPointerError::io)
        }
        CanonicalField::Extra(extra) => write_line(writer, extra.key(), extra.value()),
        CanonicalField::Oid(oid) => writer
            .write_all(b"oid ")
            .and_then(|_| writer.write_all(SHA256_PREFIX.as_bytes()))
            .and_then(|_| writer.write_all(oid.hex().as_bytes()))
            .and_then(|_| writer.write_all(b"\n"))
            .map_err(LfsPointerError::io),
        CanonicalField::Size(size) => writer
            .write_all(b"size ")
            .map_err(LfsPointerError::io)
            .and_then(|_| write_u64(writer, size))
            .and_then(|_| writer.write_all(b"\n").map_err(LfsPointerError::io)),
    }
}

fn validate_canonical_fields(pointer: &LfsPointer) -> Result<(), LfsPointerError> {
    let mut fields = pointer.canonical_fields();
    fields.sort_unstable_by(compare_canonical_fields);
    if fields
        .windows(2)
        .any(|pair| compare_canonical_fields(&pair[0], &pair[1]).is_eq())
    {
        return Err(LfsPointerError::DuplicateKey);
    }
    Ok(())
}

fn validate_serializable_values(pointer: &LfsPointer) -> Result<(), LfsPointerError> {
    for extra in &pointer.extras {
        validate_extra_value(extra.value(), true)?;
    }
    Ok(())
}

fn validate_text_controls(text: &str) -> Result<(), LfsPointerError> {
    if text
        .chars()
        .any(|character| matches!(character, '\r' | '\0'))
    {
        return Err(LfsPointerError::InvalidControl);
    }
    Ok(())
}

fn split_line(line: &str) -> Result<(&str, &str), LfsPointerError> {
    let Some((key, value)) = line.split_once(' ') else {
        return Err(LfsPointerError::InvalidLine);
    };
    if key.is_empty() || !key.bytes().all(is_valid_key_byte) {
        return Err(LfsPointerError::InvalidLine);
    }
    Ok((key, value))
}

fn validate_strict_encoding(bytes: &[u8]) -> Result<(), LfsPointerError> {
    if !bytes.ends_with(b"\n") {
        return Err(LfsPointerError::MissingFinalNewline);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| LfsPointerError::InvalidUtf8)?;
    validate_text_controls(text)?;
    let content = text
        .strip_suffix('\n')
        .ok_or(LfsPointerError::MissingFinalNewline)?;
    let mut lines = content.split('\n');
    let Some(first_line) = lines.next() else {
        return Err(LfsPointerError::MissingRequiredKey);
    };
    let (first_key, first_value) = split_line(first_line)?;
    if first_key != "version" || first_value != LFS_POINTER_VERSION {
        return Err(LfsPointerError::NonCanonicalOrder);
    }

    let mut previous_key = None;
    for line in lines {
        let (key, value) = split_line(line)?;
        if value.starts_with(' ') {
            return Err(LfsPointerError::NonCanonicalEncoding);
        }
        if previous_key.is_some_and(|previous| key <= previous) {
            return Err(LfsPointerError::NonCanonicalOrder);
        }
        previous_key = Some(key);
        if key == "size" {
            parse_size_canonical(value)?;
        } else if key.starts_with("ext-") {
            parse_extension_key(key)?;
        }
    }
    Ok(())
}

fn is_valid_key_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
}

fn parse_extension_key(key: &str) -> Result<(u32, &str), LfsPointerError> {
    let Some(suffix) = key.strip_prefix("ext-") else {
        return Err(LfsPointerError::InvalidKey);
    };
    let Some((priority, name)) = suffix.split_once('-') else {
        return Err(LfsPointerError::InvalidKey);
    };
    if priority.is_empty()
        || name.is_empty()
        || !priority.bytes().all(|byte| byte.is_ascii_digit())
        || !name.bytes().all(is_valid_key_byte)
    {
        return Err(LfsPointerError::InvalidKey);
    }
    if priority.len() != 1 {
        return Err(LfsPointerError::InvalidExtensionPriority);
    }
    let priority = priority
        .parse::<u32>()
        .map_err(|_| LfsPointerError::InvalidExtensionPriority)?;
    Ok((priority, name))
}

fn validate_extensions(extensions: &[LfsPointerExtension]) -> Result<(), LfsPointerError> {
    let mut priorities = Vec::new();
    let mut previous_key = None;
    for extension in extensions {
        let (priority, name) = parse_extension_key(extension.key())?;
        if priority != extension.priority || name != extension.name() {
            return Err(LfsPointerError::NonCanonicalEncoding);
        }
        if priorities.contains(&priority) {
            return Err(LfsPointerError::DuplicateExtensionPriority);
        }
        let canonical_key = canonical_extension_key(extension);
        if previous_key
            .as_deref()
            .is_some_and(|previous| previous >= canonical_key.as_str())
        {
            return Err(LfsPointerError::NonCanonicalOrder);
        }
        previous_key = Some(canonical_key);
        priorities.push(priority);
    }
    Ok(())
}

fn validate_extra_key(key: &str) -> Result<(), LfsPointerError> {
    if key == "version"
        || key == "oid"
        || key == "size"
        || key.starts_with("ext-")
        || !key.bytes().all(is_valid_key_byte)
    {
        return Err(LfsPointerError::InvalidKey);
    }
    Ok(())
}

fn validate_extra_value(value: &str, canonical: bool) -> Result<(), LfsPointerError> {
    if value
        .chars()
        .any(|character| matches!(character, '\r' | '\n' | '\0'))
    {
        return Err(LfsPointerError::InvalidControl);
    }
    if canonical && value.starts_with(' ') {
        return Err(LfsPointerError::NonCanonicalEncoding);
    }
    Ok(())
}

fn validate_extras(extras: &[LfsPointerExtra]) -> Result<(), LfsPointerError> {
    let mut previous = None;
    for extra in extras {
        validate_extra_key(extra.key())?;
        validate_extra_value(extra.value(), true)?;
        if previous.is_some_and(|previous| previous >= extra.key()) {
            return Err(LfsPointerError::NonCanonicalOrder);
        }
        previous = Some(extra.key());
    }
    Ok(())
}

fn parse_size_current(value: &str) -> Result<u64, LfsPointerError> {
    let size = value
        .parse::<i64>()
        .map_err(|_| LfsPointerError::InvalidSize)?;
    if size < 0 {
        return Err(LfsPointerError::InvalidSize);
    }
    checked_size(size as u64)
}

fn parse_size_canonical(value: &str) -> Result<u64, LfsPointerError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(LfsPointerError::InvalidSize);
    }
    checked_size(value.parse().map_err(|_| LfsPointerError::InvalidSize)?)
}

fn checked_size(size: u64) -> Result<u64, LfsPointerError> {
    if size > LFS_POINTER_MAX_SIZE {
        return Err(LfsPointerError::InvalidSize);
    }
    Ok(size)
}

fn line_len(key: &str, value: &str) -> Result<usize, LfsPointerError> {
    checked_add(checked_add(key.len(), 1)?, checked_add(value.len(), 1)?)
}

fn oid_line_len(key: &str) -> Result<usize, LfsPointerError> {
    checked_add(
        checked_add(key.len(), 1)?,
        checked_add(SHA256_PREFIX.len() + SHA256_HEX_BYTES, 1)?,
    )
}

fn extension_line_len(extension: &LfsPointerExtension) -> Result<usize, LfsPointerError> {
    checked_add(
        checked_add(
            checked_add(
                checked_add(b"ext-".len(), decimal_len_u32(extension.priority()))?,
                checked_add(1, extension.name().len())?,
            )?,
            1,
        )?,
        checked_add(SHA256_PREFIX.len() + SHA256_HEX_BYTES, 1)?,
    )
}

fn size_line_len(size: u64) -> Result<usize, LfsPointerError> {
    checked_add(
        checked_add("size".len(), 1)?,
        checked_add(decimal_len_u64(size), 1)?,
    )
}

fn decimal_len_u32(mut value: u32) -> usize {
    let mut length = 1;
    while value >= 10 {
        value /= 10;
        length += 1;
    }
    length
}

fn decimal_len_u64(mut value: u64) -> usize {
    let mut length = 1;
    while value >= 10 {
        value /= 10;
        length += 1;
    }
    length
}

fn checked_add(left: usize, right: usize) -> Result<usize, LfsPointerError> {
    left.checked_add(right).ok_or(LfsPointerError::TooLarge)
}

fn write_line<W: Write>(writer: &mut W, key: &str, value: &str) -> Result<(), LfsPointerError> {
    writer
        .write_all(key.as_bytes())
        .and_then(|_| writer.write_all(b" "))
        .and_then(|_| writer.write_all(value.as_bytes()))
        .and_then(|_| writer.write_all(b"\n"))
        .map_err(LfsPointerError::io)
}

fn write_u32<W: Write>(writer: &mut W, value: u32) -> Result<(), LfsPointerError> {
    write_decimal(writer, value as u64)
}

fn write_u64<W: Write>(writer: &mut W, value: u64) -> Result<(), LfsPointerError> {
    write_decimal(writer, value)
}

fn write_decimal<W: Write>(writer: &mut W, value: u64) -> Result<(), LfsPointerError> {
    let mut digits = [0; 20];
    let mut cursor = digits.len();
    let mut remaining = value;
    loop {
        cursor -= 1;
        digits[cursor] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    writer
        .write_all(&digits[cursor..])
        .map_err(LfsPointerError::io)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "sha256:4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";
    const OID_2: &str = "sha256:5d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";
    const HEX: &str = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";

    #[test]
    fn parses_official_v1_vector_strictly() {
        let bytes = format!("version {LFS_POINTER_VERSION}\noid {OID}\nsize 12345\n");
        let pointer = LfsPointer::parse_strict(bytes.as_bytes()).unwrap();
        assert_eq!(pointer.oid().hex(), HEX);
        assert_eq!(pointer.oid().bytes()[0], 0x4d);
        assert_eq!(pointer.size(), 12_345);
        assert!(pointer.extensions().is_empty());
        assert!(pointer.extras().is_empty());
        assert_eq!(pointer.to_bytes().unwrap(), bytes.as_bytes());
    }

    #[test]
    fn parses_and_preserves_sorted_official_extensions() {
        let bytes = format!(
            "version {LFS_POINTER_VERSION}\next-0-foo {OID}\next-1-bar {OID_2}\noid {OID_2}\nsize 123\n"
        );
        let pointer = LfsPointer::parse_strict(bytes.as_bytes()).unwrap();
        assert_eq!(pointer.extensions()[0].priority(), 0);
        assert_eq!(pointer.extensions()[1].priority(), 1);
        assert_eq!(pointer.extensions()[0].name(), "foo");
        assert_eq!(pointer.to_bytes().unwrap(), bytes.as_bytes());
    }

    #[test]
    fn current_parser_accepts_missing_lf_order_and_unknown_fields() {
        let bytes =
            format!("size 123\ncustom value with spaces\noid {OID}\nversion {LFS_POINTER_VERSION}");
        let pointer = LfsPointer::parse_current(bytes.as_bytes()).unwrap();
        assert_eq!(pointer.extras()[0].key(), "custom");
        assert_eq!(pointer.extras()[0].value(), "value with spaces");
        assert!(LfsPointer::parse_strict(bytes.as_bytes()).is_err());
        let canonical = pointer.to_bytes().unwrap();
        assert!(canonical.ends_with(b"\n"));
        assert!(LfsPointer::parse_strict(&canonical).is_ok());
    }

    #[test]
    fn rejects_deprecated_versions_and_empty_filter_input() {
        assert!(!LfsPointer::is_current_pointer(b""));
        let old = b"version https://hawser.github.com/spec/v1\noid sha256:\
4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393\nsize 1\n";
        assert!(!LfsPointer::is_current_pointer(old));
    }

    #[test]
    fn rejects_noncanonical_vectors() {
        let valid = format!("version {LFS_POINTER_VERSION}\noid {OID}\nsize 0\n");
        for malformed in [
            valid.replace("oid ", "OID "),
            valid.replace("size 0", "size 00"),
            valid.replace("size 0", "size +0"),
            valid.replace("oid ", "oid sha256:"),
            valid.replace("size 0", "size 0\r"),
            valid.replace("version ", "ext-0-foo "),
            format!("version {LFS_POINTER_VERSION}\nsize 0\noid {OID}\n"),
            format!("version {LFS_POINTER_VERSION}\noid {OID}\nsize 0"),
        ] {
            assert!(
                LfsPointer::parse_strict(malformed.as_bytes()).is_err(),
                "{malformed:?}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_priorities_even_with_different_names() {
        let duplicate = format!(
            "version {LFS_POINTER_VERSION}\next-1-a {OID}\next-1-b {OID_2}\noid {OID_2}\nsize 1\n"
        );
        assert_eq!(
            LfsPointer::parse_current(duplicate.as_bytes()).unwrap_err(),
            LfsPointerError::DuplicateExtensionPriority
        );
    }

    #[test]
    fn canonicalizes_global_key_order() {
        let unordered = format!(
            "version {LFS_POINTER_VERSION}\nsize 1\nzz value\next-2-two {OID_2}\next-1-one {OID}\naaa value\noid {OID_2}\n"
        );
        let pointer = LfsPointer::parse_current(unordered.as_bytes()).unwrap();
        assert_eq!(
            pointer.check_canonical().unwrap_err(),
            LfsPointerError::NonCanonicalOrder
        );
        let canonical = pointer.to_bytes().unwrap();
        let text = String::from_utf8(canonical.clone()).unwrap();
        assert!(text.find("aaa value").unwrap() < text.find("ext-1-one").unwrap());
        assert!(text.find("ext-1-one").unwrap() < text.find("ext-2-two").unwrap());
        assert!(text.find("ext-2-two").unwrap() < text.find("oid ").unwrap());
        assert!(text.find("oid ").unwrap() < text.find("size ").unwrap());
        assert!(text.find("size ").unwrap() < text.find("zz value").unwrap());
        assert!(LfsPointer::parse_strict(&canonical).is_ok());
    }

    #[test]
    fn rejects_multi_digit_extension_priorities_like_current_git_lfs() {
        let bytes =
            format!("version {LFS_POINTER_VERSION}\next-10-ten {OID}\noid {OID_2}\nsize 1\n");
        assert_eq!(
            LfsPointer::parse_current(bytes.as_bytes()).unwrap_err(),
            LfsPointerError::InvalidExtensionPriority
        );
    }

    #[test]
    fn enforces_oid_and_size_bounds() {
        assert!(LfsOid::from_hex(&HEX.to_ascii_uppercase()).is_err());
        assert!(LfsOid::from_hex("0").is_err());
        assert!(parse_size_canonical("18446744073709551616").is_err());
        assert!(parse_size_canonical("9223372036854775808").is_err());
        assert!(parse_size_canonical("01").is_err());
        assert_eq!(parse_size_canonical("0").unwrap(), 0);
        assert_eq!(
            parse_size_canonical("9223372036854775807").unwrap(),
            i64::MAX as u64
        );
    }

    #[test]
    fn only_total_pointer_size_bounds_fields() {
        let oid = LfsOid::parse(OID).unwrap();
        assert!(LfsPointerExtension::new("ext".into(), oid).is_err());
        assert!(LfsPointerExtension::new("ext-".into(), oid).is_err());
        let long_extension = format!("ext-0-{}", "x".repeat(200));
        assert!(LfsPointerExtension::new(long_extension, oid).is_ok());
        let large_value = LfsPointerExtra::new("custom".into(), "x".repeat(700)).unwrap();
        assert!(LfsPointer::new_with_extras(oid, 1, Vec::new(), vec![large_value]).is_ok());
    }

    #[test]
    fn current_parser_accepts_parse_int_size_forms_and_limited_value_restrictions() {
        for (value, expected) in [("+1", 1), ("01", 1), ("-0", 0)] {
            let bytes = format!(
                "version {LFS_POINTER_VERSION}\noid {OID}\nsize {value}\ncustom \t\u{000b}"
            );
            assert_eq!(
                LfsPointer::parse_current(bytes.as_bytes()).unwrap().size(),
                expected
            );
            assert!(LfsPointer::parse_strict(bytes.as_bytes()).is_err());
        }

        let empty = format!("version {LFS_POINTER_VERSION}\noid {OID}\nsize 1\ncustom \n");
        assert_eq!(
            LfsPointer::parse_current(empty.as_bytes())
                .unwrap()
                .extras()[0]
                .value(),
            ""
        );
        for value in ["bad\rvalue", "bad\u{0000}value"] {
            let bytes = format!("version {LFS_POINTER_VERSION}\noid {OID}\nsize 1\ncustom {value}");
            assert_eq!(
                LfsPointer::parse_current(bytes.as_bytes()).unwrap_err(),
                LfsPointerError::InvalidControl
            );
        }
    }

    #[test]
    fn canonical_values_have_exactly_one_separator_and_round_trip_strictly() {
        let oid = LfsOid::parse(OID).unwrap();
        for value in ["", "value with spaces", "\tleading", "\u{000b}control"] {
            let extra = LfsPointerExtra::new("custom".into(), value.into()).unwrap();
            let pointer = LfsPointer::new_with_extras(oid, 1, Vec::new(), vec![extra]).unwrap();
            let bytes = pointer.to_bytes().unwrap();
            assert!(LfsPointer::parse_strict(&bytes).is_ok(), "{value:?}");
        }
        for value in [" leading", "  two leading"] {
            assert_eq!(
                LfsPointerExtra::new("custom".into(), value.into()).unwrap_err(),
                LfsPointerError::NonCanonicalEncoding
            );
            let current =
                format!("version {LFS_POINTER_VERSION}\noid {OID}\nsize 1\ncustom {value}");
            let pointer = LfsPointer::parse_current(current.as_bytes()).unwrap();
            assert_eq!(pointer.extras()[0].value(), value);
            assert!(LfsPointer::parse_strict(current.as_bytes()).is_err());
            assert_eq!(
                pointer.to_bytes().unwrap_err(),
                LfsPointerError::NonCanonicalEncoding
            );
        }
    }

    #[test]
    fn accepts_1023_bytes_and_rejects_1024_bytes() {
        let oid = LfsOid::parse(OID).unwrap();
        let base = LfsPointer::new(oid, 1, Vec::new()).unwrap();
        let base_len = base.serialized_len().unwrap();
        let line_overhead = 3;
        let first_value_len = (1023 - base_len - 2 * line_overhead) / 2;
        let second_value_len = 1023 - base_len - 2 * line_overhead - first_value_len;
        let extras = vec![
            LfsPointerExtra::new("a".into(), "x".repeat(first_value_len)).unwrap(),
            LfsPointerExtra::new("b".into(), "x".repeat(second_value_len)).unwrap(),
        ];
        let pointer = LfsPointer::new_with_extras(oid, 1, Vec::new(), extras).unwrap();
        assert_eq!(pointer.serialized_len().unwrap(), 1023);
        assert_eq!(pointer.to_bytes().unwrap().len(), 1023);

        let oversized = vec![
            LfsPointerExtra::new("a".into(), "x".repeat(first_value_len + 1)).unwrap(),
            LfsPointerExtra::new("b".into(), "x".repeat(second_value_len)).unwrap(),
        ];
        assert_eq!(
            LfsPointer::new_with_extras(oid, 1, Vec::new(), oversized).unwrap_err(),
            LfsPointerError::TooLarge
        );
    }

    #[test]
    fn writes_without_a_whole_pointer_intermediate() {
        let oid = LfsOid::parse(OID).unwrap();
        let pointer = LfsPointer::new(oid, i64::MAX as u64, Vec::new()).unwrap();
        let mut output = Vec::new();
        pointer.write_to(&mut output).unwrap();
        assert_eq!(output, pointer.to_bytes().unwrap());
    }
}
