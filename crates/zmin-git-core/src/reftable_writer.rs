use std::collections::BTreeMap;
use std::io::{self, Write};

use crc32fast::Hasher as Crc32Hasher;
use flate2::{Compression, write::ZlibEncoder};

use crate::GitHashAlgorithm;

const DEFAULT_BLOCK_SIZE: usize = 4096;
const DEFAULT_RESTART_INTERVAL: usize = 16;
const FOOTER_V1_LEN: usize = 68;
const FOOTER_V2_LEN: usize = 72;
const HEADER_V1_LEN: usize = 24;
const HEADER_V2_LEN: usize = 28;
const INDEX_THRESHOLD: usize = 3;
const MAX_RESTARTS: usize = u16::MAX as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReftableWriteOptions {
    pub(crate) block_size: usize,
    pub(crate) restart_interval: usize,
    pub(crate) index_objects: bool,
}

impl Default for ReftableWriteOptions {
    fn default() -> Self {
        Self {
            block_size: DEFAULT_BLOCK_SIZE,
            restart_interval: DEFAULT_RESTART_INTERVAL,
            index_objects: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReftableEncodedRecord {
    pub(crate) key: Vec<u8>,
    pub(crate) value_type: u8,
    pub(crate) value: Vec<u8>,
    pub(crate) object_ids: Vec<Vec<u8>>,
}

#[derive(Debug)]
struct ReftableBlockDescriptor {
    offset: u64,
    last_key: Vec<u8>,
}

#[derive(Debug, Default)]
struct ReftableSectionStats {
    offset: u64,
    index_offset: u64,
    index_blocks: usize,
}

#[derive(Debug)]
struct ReftableTableWriter {
    algorithm: GitHashAlgorithm,
    min_update_index: u64,
    max_update_index: u64,
    options: ReftableWriteOptions,
    bytes: Vec<u8>,
    pending_padding: usize,
    logical_next: usize,
}

impl ReftableTableWriter {
    fn new(
        algorithm: GitHashAlgorithm,
        min_update_index: u64,
        max_update_index: u64,
        options: ReftableWriteOptions,
    ) -> Self {
        Self {
            algorithm,
            min_update_index,
            max_update_index,
            options,
            bytes: Vec::new(),
            pending_padding: 0,
            logical_next: 0,
        }
    }

    fn write_section(
        &mut self,
        block_type: u8,
        records: &[ReftableEncodedRecord],
        object_offsets: Option<&mut BTreeMap<Vec<u8>, Vec<u64>>>,
    ) -> io::Result<ReftableSectionStats> {
        if records.is_empty() {
            return Ok(ReftableSectionStats::default());
        }
        let mut stats = ReftableSectionStats {
            offset: self.logical_next as u64,
            ..ReftableSectionStats::default()
        };
        let mut blocks = self.write_blocks(block_type, records, object_offsets)?;
        while blocks.len() > INDEX_THRESHOLD {
            stats.index_offset = self.logical_next as u64;
            let index_records = blocks
                .iter()
                .map(|block| ReftableEncodedRecord {
                    key: block.last_key.clone(),
                    value_type: 0,
                    value: encode_varint_bytes(block.offset),
                    object_ids: Vec::new(),
                })
                .collect::<Vec<_>>();
            blocks = self.write_blocks(b'i', &index_records, None)?;
            stats.index_blocks += blocks.len();
        }
        Ok(stats)
    }

    fn write_blocks(
        &mut self,
        block_type: u8,
        records: &[ReftableEncodedRecord],
        mut object_offsets: Option<&mut BTreeMap<Vec<u8>, Vec<u64>>>,
    ) -> io::Result<Vec<ReftableBlockDescriptor>> {
        let mut blocks = Vec::new();
        let mut record_index = 0;
        while record_index < records.len() {
            let block_offset = self.logical_next as u64;
            let first_block = self.logical_next == 0;
            let header = first_block.then(|| {
                encode_header(
                    self.algorithm,
                    self.options.block_size,
                    self.min_update_index,
                    self.max_update_index,
                )
            });
            let mut block = ReftableBlockWriter::new(
                block_type,
                self.options.block_size,
                self.options.restart_interval,
                header,
            );
            let first_record = record_index;
            while record_index < records.len() && block.try_add(&records[record_index])? {
                if let Some(offsets) = object_offsets.as_deref_mut() {
                    for object_id in &records[record_index].object_ids {
                        let entries = offsets.entry(object_id.clone()).or_default();
                        if entries.last().copied() != Some(block_offset) {
                            entries.push(block_offset);
                        }
                    }
                }
                record_index += 1;
            }
            if record_index == first_record {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "reftable entry too large",
                ));
            }
            let last_key = records[record_index - 1].key.clone();
            let raw = block.finish()?;
            let padding = if block_type == b'g' {
                0
            } else {
                self.options.block_size.saturating_sub(raw.len())
            };
            self.append_block(raw, padding);
            blocks.push(ReftableBlockDescriptor {
                offset: block_offset,
                last_key,
            });
        }
        Ok(blocks)
    }

    fn append_block(&mut self, raw: Vec<u8>, padding: usize) {
        if self.pending_padding > 0 {
            self.bytes
                .resize(self.bytes.len() + self.pending_padding, 0);
        }
        self.pending_padding = padding;
        self.bytes.extend_from_slice(&raw);
        self.logical_next = self.bytes.len() + self.pending_padding;
    }

    fn discard_pending_padding(&mut self) {
        self.pending_padding = 0;
        self.logical_next = self.bytes.len();
    }

    fn finish(
        mut self,
        ref_stats: ReftableSectionStats,
        object_stats: ReftableSectionStats,
        object_id_len: usize,
        log_stats: ReftableSectionStats,
    ) -> Vec<u8> {
        self.discard_pending_padding();
        if self.bytes.is_empty() {
            self.bytes.extend_from_slice(&encode_header(
                self.algorithm,
                self.options.block_size,
                self.min_update_index,
                self.max_update_index,
            ));
        }
        let mut footer = encode_header(
            self.algorithm,
            self.options.block_size,
            self.min_update_index,
            self.max_update_index,
        );
        footer.extend_from_slice(&ref_stats.index_offset.to_be_bytes());
        footer
            .extend_from_slice(&((object_stats.offset << 5) | object_id_len as u64).to_be_bytes());
        footer.extend_from_slice(&object_stats.index_offset.to_be_bytes());
        footer.extend_from_slice(&log_stats.offset.to_be_bytes());
        footer.extend_from_slice(&log_stats.index_offset.to_be_bytes());
        let mut hasher = Crc32Hasher::new();
        hasher.update(&footer);
        footer.extend_from_slice(&hasher.finalize().to_be_bytes());
        debug_assert_eq!(
            footer.len(),
            match self.algorithm {
                GitHashAlgorithm::Sha1 => FOOTER_V1_LEN,
                GitHashAlgorithm::Sha256 => FOOTER_V2_LEN,
            }
        );
        self.bytes.extend_from_slice(&footer);
        self.bytes
    }
}

#[derive(Debug)]
struct ReftableBlockWriter {
    block_type: u8,
    block_size: usize,
    restart_interval: usize,
    header_offset: usize,
    bytes: Vec<u8>,
    entries: usize,
    last_key: Vec<u8>,
    restart_offsets: Vec<usize>,
}

impl ReftableBlockWriter {
    fn new(
        block_type: u8,
        block_size: usize,
        restart_interval: usize,
        header: Option<Vec<u8>>,
    ) -> Self {
        let mut bytes = header.unwrap_or_default();
        let header_offset = bytes.len();
        bytes.extend_from_slice(&[block_type, 0, 0, 0]);
        Self {
            block_type,
            block_size,
            restart_interval,
            header_offset,
            bytes,
            entries: 0,
            last_key: Vec::new(),
            restart_offsets: Vec::new(),
        }
    }

    fn try_add(&mut self, record: &ReftableEncodedRecord) -> io::Result<bool> {
        if record.key.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "reftable record key is empty",
            ));
        }
        let previous = if self.entries % self.restart_interval == 0 {
            &[][..]
        } else {
            self.last_key.as_slice()
        };
        let prefix_len = common_prefix_len(previous, &record.key);
        let mut encoded = Vec::new();
        write_varint(&mut encoded, prefix_len as u64);
        write_varint(
            &mut encoded,
            (((record.key.len() - prefix_len) as u64) << 3) | u64::from(record.value_type),
        );
        encoded.extend_from_slice(&record.key[prefix_len..]);
        encoded.extend_from_slice(&record.value);
        let is_restart = prefix_len == 0 && self.restart_offsets.len() < MAX_RESTARTS;
        let restart_count = self.restart_offsets.len() + usize::from(is_restart);
        if self.bytes.len() + encoded.len() + restart_count * 3 + 2 > self.block_size {
            return Ok(false);
        }
        if is_restart {
            self.restart_offsets.push(self.bytes.len());
        }
        self.bytes.extend_from_slice(&encoded);
        self.last_key.clone_from(&record.key);
        self.entries += 1;
        Ok(true)
    }

    fn finish(mut self) -> io::Result<Vec<u8>> {
        for offset in &self.restart_offsets {
            write_u24(&mut self.bytes, *offset)?;
        }
        self.bytes
            .extend_from_slice(&(self.restart_offsets.len() as u16).to_be_bytes());
        let inflated_len = self.bytes.len();
        write_u24_at(&mut self.bytes, self.header_offset + 1, inflated_len)?;
        if self.block_type != b'g' {
            return Ok(self.bytes);
        }
        let body_offset = self.header_offset + 4;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&self.bytes[body_offset..])?;
        let compressed = encoder.finish()?;
        self.bytes.truncate(body_offset);
        self.bytes.extend_from_slice(&compressed);
        Ok(self.bytes)
    }
}

pub(crate) fn encode_reftable(
    algorithm: GitHashAlgorithm,
    min_update_index: u64,
    max_update_index: u64,
    refs: &[ReftableEncodedRecord],
    logs: &[ReftableEncodedRecord],
    options: ReftableWriteOptions,
) -> io::Result<Vec<u8>> {
    let mut writer =
        ReftableTableWriter::new(algorithm, min_update_index, max_update_index, options);
    let mut object_offsets = BTreeMap::new();
    let ref_stats = writer.write_section(b'r', refs, Some(&mut object_offsets))?;
    let (object_records, object_id_len) = if options.index_objects && ref_stats.index_blocks > 0 {
        encode_object_records(object_offsets)
    } else {
        (Vec::new(), 0)
    };
    let object_stats = writer.write_section(b'o', &object_records, None)?;
    writer.discard_pending_padding();
    let log_stats = writer.write_section(b'g', logs, None)?;
    Ok(writer.finish(ref_stats, object_stats, object_id_len, log_stats))
}

fn encode_object_records(
    object_offsets: BTreeMap<Vec<u8>, Vec<u64>>,
) -> (Vec<ReftableEncodedRecord>, usize) {
    let mut previous: Option<&[u8]> = None;
    let mut common_max = 1;
    for object_id in object_offsets.keys() {
        if let Some(previous) = previous {
            common_max = common_max.max(common_prefix_len(previous, object_id));
        }
        previous = Some(object_id);
    }
    let object_id_len = common_max + 1;
    let records = object_offsets
        .into_iter()
        .map(|(object_id, offsets)| {
            let value_type = if (1..8).contains(&offsets.len()) {
                offsets.len() as u8
            } else {
                0
            };
            let mut value = Vec::new();
            if value_type == 0 {
                write_varint(&mut value, offsets.len() as u64);
            }
            if let Some(first) = offsets.first().copied() {
                write_varint(&mut value, first);
                for pair in offsets.windows(2) {
                    write_varint(&mut value, pair[1] - pair[0]);
                }
            }
            ReftableEncodedRecord {
                key: object_id[..object_id_len.min(object_id.len())].to_vec(),
                value_type,
                value,
                object_ids: Vec::new(),
            }
        })
        .collect();
    (records, object_id_len)
}

fn encode_header(
    algorithm: GitHashAlgorithm,
    block_size: usize,
    min_update_index: u64,
    max_update_index: u64,
) -> Vec<u8> {
    let mut header = Vec::with_capacity(match algorithm {
        GitHashAlgorithm::Sha1 => HEADER_V1_LEN,
        GitHashAlgorithm::Sha256 => HEADER_V2_LEN,
    });
    header.extend_from_slice(b"REFT");
    header.push(match algorithm {
        GitHashAlgorithm::Sha1 => 1,
        GitHashAlgorithm::Sha256 => 2,
    });
    header.extend_from_slice(&(block_size as u32).to_be_bytes()[1..]);
    header.extend_from_slice(&min_update_index.to_be_bytes());
    header.extend_from_slice(&max_update_index.to_be_bytes());
    if algorithm == GitHashAlgorithm::Sha256 {
        header.extend_from_slice(b"s256");
    }
    header
}

fn encode_varint_bytes(value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_varint(&mut bytes, value);
    bytes
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    let mut encoded = [0_u8; 10];
    let mut cursor = encoded.len() - 1;
    encoded[cursor] = (value & 0x7f) as u8;
    while value > 0x7f {
        value = (value >> 7).saturating_sub(1);
        cursor -= 1;
        encoded[cursor] = ((value & 0x7f) as u8) | 0x80;
    }
    out.extend_from_slice(&encoded[cursor..]);
}

fn common_prefix_len(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

fn write_u24(out: &mut Vec<u8>, value: usize) -> io::Result<()> {
    if value >= 1 << 24 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "reftable u24 overflow",
        ));
    }
    out.extend_from_slice(&(value as u32).to_be_bytes()[1..]);
    Ok(())
}

fn write_u24_at(out: &mut [u8], offset: usize, value: usize) -> io::Result<()> {
    if value >= 1 << 24 || offset + 3 > out.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "reftable u24 write overflow",
        ));
    }
    out[offset..offset + 3].copy_from_slice(&(value as u32).to_be_bytes()[1..]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_default_header_and_restart_intervals() {
        let records = (0..17)
            .map(|index| ReftableEncodedRecord {
                key: format!("refs/heads/branch-{index:02}").into_bytes(),
                value_type: 1,
                value: {
                    let mut value = vec![0];
                    value.extend_from_slice(&[index as u8; 20]);
                    value
                },
                object_ids: vec![vec![index as u8; 20]],
            })
            .collect::<Vec<_>>();
        let bytes = encode_reftable(
            GitHashAlgorithm::Sha1,
            1,
            1,
            &records,
            &[],
            ReftableWriteOptions::default(),
        )
        .expect("encode reftable");

        assert_eq!(&bytes[..4], b"REFT");
        assert_eq!(&bytes[5..8], &[0, 0x10, 0]);
        assert_eq!(
            u16::from_be_bytes(
                bytes[bytes.len() - FOOTER_V1_LEN - 2..bytes.len() - FOOTER_V1_LEN]
                    .try_into()
                    .expect("restart count")
            ),
            2
        );
    }
}
