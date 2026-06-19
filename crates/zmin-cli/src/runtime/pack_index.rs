use super::{CliError, Result};

pub(crate) fn push_commit_graph_chunk(out: &mut Vec<u8>, id: &[u8], offset: u64) {
    out.extend_from_slice(id);
    push_u64_be(out, offset);
}

pub(crate) fn commit_graph_chunk_range_from_offsets<'a>(
    bytes: &'a [u8],
    chunks: &[([u8; 4], usize)],
    id: &[u8; 4],
    graph_data_end: usize,
) -> Result<&'a [u8]> {
    let start = chunks
        .iter()
        .find_map(|(chunk_id, offset)| (chunk_id == id).then_some(*offset))
        .ok_or_else(|| CliError::Fatal {
            code: 1,
            message: format!(
                "commit-graph is missing {} chunk",
                String::from_utf8_lossy(id)
            ),
        })?;
    let end = chunks
        .iter()
        .map(|(_, offset)| *offset)
        .filter(|offset| *offset > start)
        .min()
        .unwrap_or(graph_data_end);
    Ok(&bytes[start..end])
}

pub(crate) fn push_u32_be(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(crate) fn push_u64_be(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

pub(crate) fn read_u32_be(bytes: &[u8]) -> Result<u32> {
    let bytes: [u8; 4] = bytes.try_into().map_err(|_| CliError::Fatal {
        code: 1,
        message: "truncated big-endian u32".into(),
    })?;
    Ok(u32::from_be_bytes(bytes))
}

pub(crate) fn read_u64_be(bytes: &[u8]) -> Result<u64> {
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| CliError::Fatal {
        code: 1,
        message: "truncated big-endian u64".into(),
    })?;
    Ok(u64::from_be_bytes(bytes))
}
