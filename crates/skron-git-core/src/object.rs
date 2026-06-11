use std::fmt;
use std::io::{self, Write};

use sha1::{Digest as Sha1Digest, Sha1};
use sha2::Sha256;

/// Git repository object hash algorithm.
///
/// SHA-1 is required for compatibility with the overwhelming majority of
/// repositories. SHA-256 is modeled at the same API level so the object database
/// does not bake SHA-1 assumptions into storage or transport primitives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitHashAlgorithm {
    Sha1,
    Sha256,
}

impl GitHashAlgorithm {
    pub const fn digest_len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
        }
    }
}

/// Canonical Git object kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitObjectKind {
    Blob,
    Tree,
    Commit,
    Tag,
}

impl GitObjectKind {
    pub const fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Blob => b"blob",
            Self::Tree => b"tree",
            Self::Commit => b"commit",
            Self::Tag => b"tag",
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Tree => "tree",
            Self::Commit => "commit",
            Self::Tag => "tag",
        }
    }

    pub fn parse(bytes: &[u8]) -> Option<Self> {
        match bytes {
            b"blob" => Some(Self::Blob),
            b"tree" => Some(Self::Tree),
            b"commit" => Some(Self::Commit),
            b"tag" => Some(Self::Tag),
            _ => None,
        }
    }
}

/// Fixed-capacity object id for SHA-1 and SHA-256 Git repositories.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ObjectId {
    algorithm: GitHashAlgorithm,
    bytes: [u8; 32],
}

impl ObjectId {
    pub fn new(algorithm: GitHashAlgorithm, digest: &[u8]) -> Self {
        assert_eq!(digest.len(), algorithm.digest_len());
        let mut bytes = [0_u8; 32];
        bytes[..digest.len()].copy_from_slice(digest);
        Self { algorithm, bytes }
    }

    pub fn from_hex(algorithm: GitHashAlgorithm, hex_id: &str) -> io::Result<Self> {
        Self::from_hex_bytes(algorithm, hex_id.as_bytes())
    }

    pub fn from_hex_bytes(algorithm: GitHashAlgorithm, hex_id: &[u8]) -> io::Result<Self> {
        if hex_id.len() != algorithm.digest_len() * 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "git object id has the wrong length",
            ));
        }
        let mut bytes = [0_u8; 32];
        for (idx, pair) in hex_id.chunks_exact(2).enumerate() {
            let high = hex_nibble(pair[0])?;
            let low = hex_nibble(pair[1])?;
            bytes[idx] = (high << 4) | low;
        }
        Ok(Self::new(algorithm, &bytes[..algorithm.digest_len()]))
    }

    pub const fn algorithm(&self) -> GitHashAlgorithm {
        self.algorithm
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.algorithm.digest_len()]
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.as_bytes())
    }

    pub fn hex_len(&self) -> usize {
        self.as_bytes().len() * 2
    }

    pub fn write_hex<W: fmt::Write>(&self, writer: &mut W) -> fmt::Result {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in self.as_bytes() {
            writer.write_char(HEX[(byte >> 4) as usize] as char)?;
            writer.write_char(HEX[(byte & 0x0f) as usize] as char)?;
        }
        Ok(())
    }

    pub fn write_hex_bytes(&self, out: &mut Vec<u8>) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        out.reserve(self.hex_len());
        for byte in self.as_bytes() {
            out.push(HEX[(byte >> 4) as usize]);
            out.push(HEX[(byte & 0x0f) as usize]);
        }
    }

    pub fn write_hex_io<W: Write + ?Sized>(&self, out: &mut W) -> io::Result<()> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut buffer = [0_u8; 64];
        let mut cursor = 0;
        for byte in self.as_bytes() {
            buffer[cursor] = HEX[(byte >> 4) as usize];
            buffer[cursor + 1] = HEX[(byte & 0x0f) as usize];
            cursor += 2;
        }
        out.write_all(&buffer[..cursor])
    }

    pub fn short_hex(&self, len: usize) -> String {
        let len = len.min(self.hex_len());
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = Vec::with_capacity(len);
        for byte in self.as_bytes() {
            if out.len() == len {
                break;
            }
            out.push(HEX[(byte >> 4) as usize]);
            if out.len() == len {
                break;
            }
            out.push(HEX[(byte & 0x0f) as usize]);
        }
        String::from_utf8(out).expect("hex alphabet is valid UTF-8")
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectId {{ algorithm: {:?}, hex: \"", self.algorithm)?;
        self.write_hex(f)?;
        f.write_str("\" }")
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_hex(f)
    }
}

fn hex_nibble(byte: u8) -> io::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "git object id contains non-hex characters",
        )),
    }
}

enum GitHasherInner {
    Sha1(Sha1),
    Sha256(Sha256),
}

/// Streaming Git object hasher.
pub struct GitObjectHash {
    algorithm: GitHashAlgorithm,
    inner: GitHasherInner,
}

impl GitObjectHash {
    pub fn new(algorithm: GitHashAlgorithm) -> Self {
        let inner = match algorithm {
            GitHashAlgorithm::Sha1 => GitHasherInner::Sha1(Sha1::new()),
            GitHashAlgorithm::Sha256 => GitHasherInner::Sha256(Sha256::new()),
        };
        Self { algorithm, inner }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        match &mut self.inner {
            GitHasherInner::Sha1(hasher) => hasher.update(bytes),
            GitHasherInner::Sha256(hasher) => hasher.update(bytes),
        }
    }

    pub fn update_object_header(&mut self, kind: GitObjectKind, content_len: usize) {
        update_header(self, kind, content_len);
    }

    pub fn finalize(self) -> ObjectId {
        match self.inner {
            GitHasherInner::Sha1(hasher) => {
                let digest = hasher.finalize();
                ObjectId::new(self.algorithm, digest.as_slice())
            }
            GitHasherInner::Sha256(hasher) => {
                let digest = hasher.finalize();
                ObjectId::new(self.algorithm, digest.as_slice())
            }
        }
    }
}

/// Writer wrapper that hashes exactly the bytes written through it.
pub struct GitObjectWriter<W> {
    inner: W,
    hasher: GitObjectHash,
}

impl<W> GitObjectWriter<W> {
    pub fn new(inner: W, algorithm: GitHashAlgorithm) -> Self {
        Self {
            inner,
            hasher: GitObjectHash::new(algorithm),
        }
    }

    pub fn finish(self) -> (W, ObjectId) {
        (self.inner, self.hasher.finalize())
    }
}

impl<W: Write> Write for GitObjectWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.hasher.update(&buf[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub fn encoded_object_len(kind: GitObjectKind, content_len: usize) -> usize {
    kind.as_bytes().len() + 1 + decimal_len(content_len) + 1 + content_len
}

pub fn decode_object_header(encoded: &[u8]) -> io::Result<(GitObjectKind, usize, usize)> {
    let nul = encoded.iter().position(|byte| *byte == 0).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "git object header missing NUL")
    })?;
    let header = &encoded[..nul];
    let space = header
        .iter()
        .position(|byte| *byte == b' ')
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "git object header missing space",
            )
        })?;
    let kind = GitObjectKind::parse(&header[..space])
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown git object type"))?;
    let size = parse_decimal(&header[space + 1..])?;
    let content_start = nul + 1;
    let actual_size = encoded.len().saturating_sub(content_start);
    if actual_size != size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git object size does not match header",
        ));
    }
    Ok((kind, size, content_start))
}

/// Hash a Git object using canonical framing: `<kind> <len>\0<content>`.
pub fn hash_object(algorithm: GitHashAlgorithm, kind: GitObjectKind, content: &[u8]) -> ObjectId {
    let mut hasher = GitObjectHash::new(algorithm);
    update_header(&mut hasher, kind, content.len());
    hasher.update(content);
    hasher.finalize()
}

/// Write canonical Git object bytes and return the object id.
pub fn write_encoded_object<W: Write>(
    writer: W,
    algorithm: GitHashAlgorithm,
    kind: GitObjectKind,
    content: &[u8],
) -> io::Result<(W, ObjectId)> {
    let mut writer = GitObjectWriter::new(writer, algorithm);
    write_header(&mut writer, kind, content.len())?;
    writer.write_all(content)?;
    Ok(writer.finish())
}

fn update_header(hasher: &mut GitObjectHash, kind: GitObjectKind, content_len: usize) {
    hasher.update(kind.as_bytes());
    hasher.update(b" ");
    update_decimal(hasher, content_len);
    hasher.update(b"\0");
}

fn write_header<W: Write>(
    writer: &mut W,
    kind: GitObjectKind,
    content_len: usize,
) -> io::Result<()> {
    writer.write_all(kind.as_bytes())?;
    writer.write_all(b" ")?;
    write_decimal(writer, content_len)?;
    writer.write_all(b"\0")?;
    Ok(())
}

fn update_decimal(hasher: &mut GitObjectHash, mut value: usize) {
    let mut buf = [0_u8; 20];
    let mut cursor = buf.len();
    if value == 0 {
        cursor -= 1;
        buf[cursor] = b'0';
    } else {
        while value > 0 {
            cursor -= 1;
            buf[cursor] = b'0' + (value % 10) as u8;
            value /= 10;
        }
    }
    hasher.update(&buf[cursor..]);
}

fn write_decimal<W: Write>(writer: &mut W, mut value: usize) -> io::Result<()> {
    let mut buf = [0_u8; 20];
    let mut cursor = buf.len();
    if value == 0 {
        cursor -= 1;
        buf[cursor] = b'0';
    } else {
        while value > 0 {
            cursor -= 1;
            buf[cursor] = b'0' + (value % 10) as u8;
            value /= 10;
        }
    }
    writer.write_all(&buf[cursor..])
}

fn decimal_len(mut value: usize) -> usize {
    if value == 0 {
        return 1;
    }
    let mut len = 0;
    while value > 0 {
        len += 1;
        value /= 10;
    }
    len
}

fn parse_decimal(bytes: &[u8]) -> io::Result<usize> {
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "empty git object size",
        ));
    }
    if bytes.len() > 1 && bytes[0] == b'0' {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git object size has leading zero",
        ));
    }
    let mut value = 0_usize;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid git object size digit",
            ));
        }
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add((byte - b'0') as usize))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "git object too large"))?;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    use super::*;

    #[test]
    fn sha1_empty_blob_matches_git_known_id() {
        let id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"");
        assert_eq!(id.to_hex(), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    }

    #[test]
    fn sha1_blob_matches_git_hash_object() {
        let content = b"hello from skron\n";
        let expected = git_hash_object("blob", content);
        let actual = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, content);
        assert_eq!(actual.to_hex(), expected);
    }

    #[test]
    fn encoded_writer_emits_canonical_object_frame() {
        let content = b"abc";
        let mut encoded = Vec::new();
        let (_, id) = write_encoded_object(
            &mut encoded,
            GitHashAlgorithm::Sha1,
            GitObjectKind::Blob,
            content,
        )
        .expect("write object");

        assert_eq!(encoded, b"blob 3\0abc");
        assert_eq!(id.to_hex(), git_hash_object("blob", content));
        assert_eq!(
            encoded.len(),
            encoded_object_len(GitObjectKind::Blob, content.len())
        );
    }

    #[test]
    fn streaming_hash_matches_one_shot_hash() {
        let content = b"streamed content";
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
        update_header(&mut hasher, GitObjectKind::Blob, content.len());
        for chunk in content.chunks(3) {
            hasher.update(chunk);
        }

        let streamed = hasher.finalize();
        let one_shot = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, content);
        assert_eq!(streamed, one_shot);
    }

    #[test]
    fn sha256_object_id_uses_32_bytes() {
        let id = hash_object(GitHashAlgorithm::Sha256, GitObjectKind::Blob, b"abc");
        assert_eq!(id.algorithm(), GitHashAlgorithm::Sha256);
        assert_eq!(id.as_bytes().len(), 32);
        assert_eq!(id.to_hex().len(), 64);
    }

    #[test]
    fn parses_hex_object_id() {
        let id = ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
        )
        .expect("parse object id");
        assert_eq!(id.as_bytes().len(), 20);
        assert_eq!(id.to_hex(), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    }

    fn git_hash_object(kind: &str, content: &[u8]) -> String {
        let mut child = Command::new("git")
            .args(["hash-object", "-t", kind, "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn git hash-object");

        child
            .stdin
            .as_mut()
            .expect("git stdin")
            .write_all(content)
            .expect("write git stdin");

        let output = child.wait_with_output().expect("wait git hash-object");
        assert!(
            output.status.success(),
            "git hash-object failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git hash output utf8")
            .trim()
            .to_owned()
    }
}
