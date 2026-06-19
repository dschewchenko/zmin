use std::collections::HashSet;
use std::fs;
use std::path::Path;

use zmin_git_core::{GitHashAlgorithm, ObjectId};

use super::{
    CliError, GitRepo, Result, commit_graph_chunk_range_from_offsets, read_u32_be, read_u64_be,
};

const GRAPH_PARENT_NONE: u32 = 0x7000_0000;
const GRAPH_POSITION_MASK: u32 = 0x7fff_ffff;
const GRAPH_EXTRA_EDGE_LIST: u32 = 0x8000_0000;
const GRAPH_LAST_EDGE: u32 = 0x8000_0000;

#[derive(Debug)]
pub(crate) struct CommitGraphIndex {
    bytes: Vec<u8>,
    oidf_start: usize,
    oidl_start: usize,
    cdat_start: usize,
    edge: Option<(usize, usize)>,
    count: usize,
    digest_len: usize,
}

impl CommitGraphIndex {
    pub(crate) fn open(repo: &GitRepo) -> Result<Option<Self>> {
        let path = repo.git_dir.join("objects/info/commit-graph");
        if !Path::new(&path).is_file() {
            return Ok(None);
        }
        Self::from_bytes(fs::read(path)?).map(Some)
    }

    fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let digest_len = GitHashAlgorithm::Sha1.digest_len();
        if bytes.len() < 8 + 12 + digest_len {
            return Err(CliError::Fatal {
                code: 1,
                message: "commit-graph file is too small".into(),
            });
        }
        if &bytes[..4] != b"CGPH" || bytes[4] != 1 || bytes[5] != 1 || bytes[7] != 0 {
            return Err(CliError::Fatal {
                code: 1,
                message: "unsupported commit-graph header".into(),
            });
        }

        let chunk_count = bytes[6] as usize;
        let lookup_end = 8 + (chunk_count + 1) * 12;
        if lookup_end > bytes.len().saturating_sub(digest_len) {
            return Err(CliError::Fatal {
                code: 1,
                message: "commit-graph chunk lookup is truncated".into(),
            });
        }
        let graph_data_end = bytes.len() - digest_len;
        let mut chunks = Vec::with_capacity(chunk_count);
        let mut previous_offset = 0_u64;
        for idx in 0..=chunk_count {
            let cursor = 8 + idx * 12;
            let chunk_id = [
                bytes[cursor],
                bytes[cursor + 1],
                bytes[cursor + 2],
                bytes[cursor + 3],
            ];
            let offset = read_u64_be(&bytes[cursor + 4..cursor + 12])?;
            if offset < previous_offset || offset as usize > graph_data_end {
                return Err(CliError::Fatal {
                    code: 1,
                    message: "commit-graph chunk offsets are invalid".into(),
                });
            }
            previous_offset = offset;
            if idx < chunk_count {
                chunks.push((chunk_id, offset as usize));
            }
        }

        let oidf = commit_graph_chunk_range_from_offsets(&bytes, &chunks, b"OIDF", graph_data_end)?;
        if oidf.len() < 256 * 4 {
            return Err(CliError::Fatal {
                code: 1,
                message: "commit-graph OIDF chunk is truncated".into(),
            });
        }
        let count = read_u32_be(&oidf[255 * 4..256 * 4])? as usize;
        let oidl = commit_graph_chunk_range_from_offsets(&bytes, &chunks, b"OIDL", graph_data_end)?;
        let cdat = commit_graph_chunk_range_from_offsets(&bytes, &chunks, b"CDAT", graph_data_end)?;
        if oidl.len() < count * digest_len || cdat.len() < count * (digest_len + 16) {
            return Err(CliError::Fatal {
                code: 1,
                message: "commit-graph object chunks are truncated".into(),
            });
        }
        let edge = chunks
            .iter()
            .find_map(|(chunk_id, start)| (chunk_id == b"EDGE").then_some(*start))
            .map(|start| {
                let end = chunks
                    .iter()
                    .map(|(_, offset)| *offset)
                    .filter(|offset| *offset > start)
                    .min()
                    .unwrap_or(graph_data_end);
                (start, end)
            });

        Ok(Self {
            oidf_start: oidf.as_ptr() as usize - bytes.as_ptr() as usize,
            oidl_start: oidl.as_ptr() as usize - bytes.as_ptr() as usize,
            cdat_start: cdat.as_ptr() as usize - bytes.as_ptr() as usize,
            bytes,
            edge,
            count,
            digest_len,
        })
    }

    pub(crate) fn position(&self, id: &ObjectId) -> Option<u32> {
        if id.algorithm() != GitHashAlgorithm::Sha1 {
            return None;
        }
        let id_bytes = id.as_bytes();
        let first = id_bytes.first().copied()? as usize;
        let start = if first == 0 {
            0
        } else {
            self.fanout_count(first - 1)? as usize
        };
        let end = self.fanout_count(first)? as usize;
        let oidl = self.oidl();
        let mut low = start;
        let mut high = end;
        while low < high {
            let mid = low + (high - low) / 2;
            let mid_id = &oidl[mid * self.digest_len..(mid + 1) * self.digest_len];
            match mid_id.cmp(id_bytes) {
                std::cmp::Ordering::Less => low = mid + 1,
                std::cmp::Ordering::Greater => high = mid,
                std::cmp::Ordering::Equal => return u32::try_from(mid).ok(),
            }
        }
        None
    }

    pub(crate) fn id_at(&self, position: u32) -> Option<ObjectId> {
        let position = position as usize;
        if position >= self.count {
            return None;
        }
        let start = position * self.digest_len;
        Some(ObjectId::new(
            GitHashAlgorithm::Sha1,
            &self.oidl()[start..start + self.digest_len],
        ))
    }

    pub(crate) fn first_parent_after(
        &self,
        start: &ObjectId,
        generations: usize,
    ) -> Result<Option<ObjectId>> {
        let Some(mut position) = self.position(start) else {
            return Ok(None);
        };
        let mut parents = Vec::with_capacity(2);
        for _ in 0..generations {
            self.parent_positions(position, &mut parents)?;
            let Some(parent) = parents.first().copied() else {
                return Ok(None);
            };
            position = parent;
        }
        Ok(self.id_at(position))
    }

    pub(crate) fn generation(&self, position: u32) -> Option<u32> {
        let row = self.commit_data_row(position)?;
        let value = read_u32_be(&row[self.digest_len + 8..self.digest_len + 12]).ok()?;
        Some(value >> 2)
    }

    pub(crate) fn parent_positions(&self, position: u32, out: &mut Vec<u32>) -> Result<()> {
        out.clear();
        let row = self
            .commit_data_row(position)
            .ok_or_else(|| CliError::Fatal {
                code: 1,
                message: "commit-graph position is out of bounds".into(),
            })?;
        let first = read_u32_be(&row[self.digest_len..self.digest_len + 4])?;
        let second = read_u32_be(&row[self.digest_len + 4..self.digest_len + 8])?;
        if first != GRAPH_PARENT_NONE {
            out.push(first & GRAPH_POSITION_MASK);
        }
        if second == GRAPH_PARENT_NONE {
            return Ok(());
        }
        if second & GRAPH_EXTRA_EDGE_LIST == 0 {
            out.push(second & GRAPH_POSITION_MASK);
            return Ok(());
        }
        let Some((edge_start, edge_end)) = self.edge else {
            return Err(CliError::Fatal {
                code: 1,
                message: "commit-graph is missing EDGE chunk".into(),
            });
        };
        let mut cursor = edge_start + ((second & GRAPH_POSITION_MASK) as usize) * 4;
        while cursor + 4 <= edge_end {
            let edge = read_u32_be(&self.bytes[cursor..cursor + 4])?;
            out.push(edge & GRAPH_POSITION_MASK);
            cursor += 4;
            if edge & GRAPH_LAST_EDGE != 0 {
                return Ok(());
            }
        }
        Err(CliError::Fatal {
            code: 1,
            message: "commit-graph EDGE chunk is truncated".into(),
        })
    }

    pub(crate) fn is_ancestor(
        &self,
        ancestor: &ObjectId,
        descendant: &ObjectId,
    ) -> Result<Option<bool>> {
        let Some(ancestor_pos) = self.position(ancestor) else {
            return Ok(None);
        };
        let Some(descendant_pos) = self.position(descendant) else {
            return Ok(None);
        };
        if ancestor_pos == descendant_pos {
            return Ok(Some(true));
        }
        let Some(ancestor_generation) = self.generation(ancestor_pos) else {
            return Ok(None);
        };
        let Some(descendant_generation) = self.generation(descendant_pos) else {
            return Ok(None);
        };
        if descendant_generation < ancestor_generation {
            return Ok(Some(false));
        }

        let mut stack = Vec::with_capacity(128);
        let mut seen: Option<HashSet<u32>> = None;
        let mut parents = Vec::with_capacity(2);
        let mut current = descendant_pos;
        loop {
            self.parent_positions(current, &mut parents)?;
            if parents.is_empty() {
                return Ok(Some(false));
            }
            if parents.len() == 1 {
                let parent = parents[0];
                if parent == ancestor_pos {
                    return Ok(Some(true));
                }
                if self
                    .generation(parent)
                    .is_some_and(|generation| generation < ancestor_generation)
                {
                    return Ok(Some(false));
                }
                if let Some(seen) = seen.as_mut() {
                    if !seen.insert(parent) {
                        break;
                    }
                }
                current = parent;
                continue;
            }

            let seen_nodes = seen.get_or_insert_with(|| HashSet::with_capacity(128));
            if !seen_nodes.insert(current) {
                break;
            }
            for parent in &parents {
                if *parent == ancestor_pos {
                    return Ok(Some(true));
                }
                if self
                    .generation(*parent)
                    .is_some_and(|generation| generation >= ancestor_generation)
                    && seen_nodes.insert(*parent)
                {
                    stack.push(*parent);
                }
            }
            break;
        }

        while let Some(position) = stack.pop() {
            self.parent_positions(position, &mut parents)?;
            let Some(seen_nodes) = seen.as_mut() else {
                break;
            };
            for parent in &parents {
                if *parent == ancestor_pos {
                    return Ok(Some(true));
                }
                if self
                    .generation(*parent)
                    .is_some_and(|generation| generation >= ancestor_generation)
                    && seen_nodes.insert(*parent)
                {
                    stack.push(*parent);
                }
            }
        }
        Ok(Some(false))
    }

    fn fanout_count(&self, index: usize) -> Option<u32> {
        let start = self.oidf_start + index * 4;
        read_u32_be(&self.bytes[start..start + 4]).ok()
    }

    fn oidl(&self) -> &[u8] {
        &self.bytes[self.oidl_start..self.oidl_start + self.count * self.digest_len]
    }

    fn commit_data_row(&self, position: u32) -> Option<&[u8]> {
        let position = position as usize;
        if position >= self.count {
            return None;
        }
        let row_len = self.digest_len + 16;
        let start = self.cdat_start + position * row_len;
        Some(&self.bytes[start..start + row_len])
    }
}
