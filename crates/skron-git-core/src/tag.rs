use std::io;

use crate::{GitHashAlgorithm, GitObjectKind, ObjectId, Signature};

const TAG_ENCODE_INITIAL_CAPACITY_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagObject {
    pub target: ObjectId,
    pub target_kind: GitObjectKind,
    pub name: Vec<u8>,
    pub tagger: Vec<u8>,
    pub message: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TagBuilder {
    target: ObjectId,
    target_kind: GitObjectKind,
    name: String,
    tagger: Signature,
    message: Vec<u8>,
}

impl TagBuilder {
    pub fn new(
        target: ObjectId,
        target_kind: GitObjectKind,
        name: impl Into<String>,
        tagger: Signature,
    ) -> io::Result<Self> {
        let name = name.into();
        validate_tag_name(&name)?;
        Ok(Self {
            target,
            target_kind,
            name,
            tagger,
            message: Vec::new(),
        })
    }

    pub fn message(mut self, message: impl Into<Vec<u8>>) -> io::Result<Self> {
        let message = message.into();
        if message.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "tag message contains NUL",
            ));
        }
        self.message = message;
        Ok(self)
    }

    pub fn encode(&self) -> io::Result<Vec<u8>> {
        encode_tag(
            &self.target,
            self.target_kind,
            &self.name,
            &self.tagger,
            &self.message,
        )
    }
}

pub fn encode_tag(
    target: &ObjectId,
    target_kind: GitObjectKind,
    name: &str,
    tagger: &Signature,
    message: &[u8],
) -> io::Result<Vec<u8>> {
    validate_tag_name(name)?;
    if message.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tag message contains NUL",
        ));
    }

    let mut out = Vec::with_capacity(tag_encode_initial_capacity(
        target,
        target_kind,
        name,
        tagger,
        message,
    ));
    out.extend_from_slice(b"object ");
    target.write_hex_bytes(&mut out);
    out.push(b'\n');
    out.extend_from_slice(b"type ");
    out.extend_from_slice(target_kind.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(b"tag ");
    out.extend_from_slice(name.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(b"tagger ");
    tagger.write_to(&mut out);
    out.extend_from_slice(b"\n\n");
    out.extend_from_slice(message);
    Ok(out)
}

fn tag_encode_initial_capacity(
    target: &ObjectId,
    target_kind: GitObjectKind,
    name: &str,
    tagger: &Signature,
    message: &[u8],
) -> usize {
    let bytes = 8_usize
        .saturating_add(target.hex_len())
        .saturating_add(6)
        .saturating_add(target_kind.as_bytes().len())
        .saturating_add(5)
        .saturating_add(name.len())
        .saturating_add(9)
        .saturating_add(tagger.encoded_len())
        .saturating_add(message.len());
    bytes.min(TAG_ENCODE_INITIAL_CAPACITY_LIMIT)
}

pub fn decode_tag(algorithm: GitHashAlgorithm, bytes: &[u8]) -> io::Result<TagObject> {
    let message_start = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|idx| idx + 2)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tag missing header end"))?;
    let headers = &bytes[..message_start - 2];
    let message = bytes[message_start..].to_vec();

    let mut target = None;
    let mut target_kind = None;
    let mut name = None;
    let mut tagger = None;

    for line in headers.split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"object ") {
            target = Some(parse_tag_id(algorithm, value)?);
        } else if let Some(value) = line.strip_prefix(b"type ") {
            target_kind = Some(GitObjectKind::parse(value).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "tag object type is invalid")
            })?);
        } else if let Some(value) = line.strip_prefix(b"tag ") {
            name = Some(value.to_vec());
        } else if let Some(value) = line.strip_prefix(b"tagger ") {
            tagger = Some(value.to_vec());
        }
    }

    Ok(TagObject {
        target: target
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tag missing object"))?,
        target_kind: target_kind
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tag missing type"))?,
        name: name.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tag missing name"))?,
        tagger: tagger
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tag missing tagger"))?,
        message,
    })
}

fn parse_tag_id(algorithm: GitHashAlgorithm, value: &[u8]) -> io::Result<ObjectId> {
    let value = std::str::from_utf8(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "tag object id is not utf-8"))?;
    ObjectId::from_hex(algorithm, value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "tag object id is invalid"))
}

fn validate_tag_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name.contains('\0') || name.contains('\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tag name contains invalid characters",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GitObjectKind, hash_object};

    #[test]
    fn tag_encode_initial_capacity_is_bounded() {
        let target = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"hello\n");
        let tagger = Signature::new(
            "Skron Test",
            "skron@example.invalid",
            1_700_000_001,
            "+0000",
        )
        .expect("tagger");
        let message = b"tag message\n";
        let expected = encode_tag(&target, GitObjectKind::Blob, "v1", &tagger, message)
            .expect("encode tag")
            .len();

        assert_eq!(
            tag_encode_initial_capacity(&target, GitObjectKind::Blob, "v1", &tagger, message),
            expected
        );

        let large_message = vec![b'a'; TAG_ENCODE_INITIAL_CAPACITY_LIMIT];
        assert_eq!(
            tag_encode_initial_capacity(
                &target,
                GitObjectKind::Blob,
                "v1",
                &tagger,
                &large_message
            ),
            TAG_ENCODE_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn encode_and_decode_annotated_tag_object() {
        let target = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"hello\n");
        let tagger = Signature::new(
            "Skron Test",
            "skron@example.invalid",
            1_700_000_001,
            "+0000",
        )
        .expect("signature");
        let encoded = TagBuilder::new(target.clone(), GitObjectKind::Blob, "v1", tagger)
            .expect("tag builder")
            .message(b"tag message\n".to_vec())
            .expect("message")
            .encode()
            .expect("encode tag");

        assert_eq!(
            encoded,
            format!(
                "object {}\ntype blob\ntag v1\ntagger Skron Test <skron@example.invalid> 1700000001 +0000\n\ntag message\n",
                target.to_hex()
            )
            .into_bytes()
        );
        assert_eq!(
            decode_tag(GitHashAlgorithm::Sha1, &encoded).expect("decode tag"),
            TagObject {
                target,
                target_kind: GitObjectKind::Blob,
                name: b"v1".to_vec(),
                tagger: b"Skron Test <skron@example.invalid> 1700000001 +0000".to_vec(),
                message: b"tag message\n".to_vec(),
            }
        );
    }
}
