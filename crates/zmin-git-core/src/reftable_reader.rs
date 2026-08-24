use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet, VecDeque};
use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

use crc32fast::Hasher as Crc32Hasher;
use flate2::read::ZlibDecoder;

use crate::refs::{ReftableLogRecord, ReftableLogSummary};
use crate::{GitHashAlgorithm, ObjectId};

const HEADER_V1_LEN: usize = 24;
const HEADER_V2_LEN: usize = 28;
const BLOCK_HEADER_LEN: usize = 4;
const FOOTER_V1_LEN: usize = 68;
const FOOTER_V2_LEN: usize = 72;
const MAX_BLOCK_LEN: usize = 0x00ff_ffff;
const MAX_TABLES_LIST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TABLE_NAME_BYTES: usize = 512;
const IO_BUFFER_BYTES: usize = 64 * 1024;

#[cfg(unix)]
use std::os::unix::fs::FileExt;
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, ReOpenFile,
};

#[derive(Debug)]
pub(crate) struct ReftableStackSnapshot {
    tables: Vec<ReftableTable>,
}

#[derive(Debug)]
pub(crate) struct ReftableTable {
    name: String,
    file: File,
    len: u64,
    identity: FileIdentity,
    algorithm: GitHashAlgorithm,
    header: Header,
    footer: Footer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileIdentity {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ReftableParsedLogRecord {
    Update(ReftableLogRecord),
    Deletion { ref_name: String, update_index: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReftableLogOrder {
    OldestToNewest,
    NewestToOldest,
}

#[derive(Debug, Clone)]
pub(crate) enum ReftableParsedRefTarget {
    Direct(ObjectId),
    Symbolic(String),
}

impl ReftableParsedLogRecord {
    pub(crate) fn key(&self) -> (String, u64) {
        match self {
            Self::Update(record) => (record.ref_name.clone(), record.update_index),
            Self::Deletion {
                ref_name,
                update_index,
            } => (ref_name.clone(), *update_index),
        }
    }
}

impl ReftableStackSnapshot {
    pub(crate) fn tables(&self) -> &[ReftableTable] {
        &self.tables
    }

    pub(crate) fn table(&self, name: &str) -> Option<&ReftableTable> {
        self.tables.iter().find(|table| table.name == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    version: u8,
    len: usize,
    block_size: usize,
    min_update_index: u64,
    max_update_index: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Footer {
    ref_index_position: u64,
    obj_position: u64,
    obj_id_len: usize,
    obj_index_position: u64,
    log_position: u64,
    log_index_position: u64,
    start: u64,
}

#[derive(Debug, Clone, Copy)]
enum FixedSection {
    Ref,
    Index,
    Object,
}

#[derive(Debug)]
struct IndexBlock {
    position: u64,
    targets: Vec<u64>,
}

#[derive(Debug, Default)]
struct LogScanResult {
    first_index_position: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct ReftableLogCursor<'a> {
    order: ReftableLogOrder,
    tables: Vec<ReftableTableLogCursor<'a>>,
    heads: Vec<Option<ReftableLogItem>>,
    heap: BinaryHeap<ReftableLogHeapEntry>,
    heap_initialized: bool,
    terminal_error: Option<ReftableLogTerminalError>,
}

#[derive(Debug)]
struct ReftableTableLogCursor<'a> {
    table: &'a ReftableTable,
    ref_name: String,
    position_cursor: ReftableLogPositionCursor,
    end: u64,
    finished: bool,
    pending: VecDeque<ReftableLogItem>,
    order: ReftableLogOrder,
    include_existence_markers: bool,
    #[cfg(test)]
    index_stats: IndexTraversalStats,
}

#[derive(Debug)]
enum ReftableLogItem {
    Summary(ReftableLogSummary),
    Tombstone { update_index: u64 },
}

#[derive(Debug, Clone)]
struct ReftableLogTerminalError {
    kind: io::ErrorKind,
    message: String,
}

impl ReftableLogTerminalError {
    fn from_error(error: &io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }

    fn into_io_error(&self) -> io::Error {
        io::Error::new(self.kind, self.message.clone())
    }
}

impl ReftableLogItem {
    fn update_index(&self) -> u64 {
        match self {
            Self::Summary(summary) => summary.update_index,
            Self::Tombstone { update_index } => *update_index,
        }
    }

    fn is_tombstone(&self) -> bool {
        matches!(self, Self::Tombstone { .. })
    }
}

#[derive(Debug, Eq, PartialEq)]
struct ReftableLogHeapEntry {
    order: ReftableLogOrder,
    table_index: usize,
    table_rank: usize,
    update_index: u64,
    tombstone: bool,
}

impl Ord for ReftableLogHeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        let update_order = match self.order {
            ReftableLogOrder::OldestToNewest => other.update_index.cmp(&self.update_index),
            ReftableLogOrder::NewestToOldest => self.update_index.cmp(&other.update_index),
        };
        update_order
            .then_with(|| self.table_rank.cmp(&other.table_rank))
            .then_with(|| self.tombstone.cmp(&other.tombstone))
            .then_with(|| self.table_index.cmp(&other.table_index))
    }
}

impl PartialOrd for ReftableLogHeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy)]
enum IndexTraversalEvent {
    BlockInitialized(u64),
    EntryDecoded(u64, usize),
}

#[cfg(test)]
#[derive(Debug, Default, Clone)]
struct IndexTraversalStats {
    block_initializations: usize,
    initialized_blocks: HashSet<u64>,
    entry_decodes: usize,
    decoded_entries: HashSet<(u64, usize)>,
}

#[cfg(test)]
impl IndexTraversalStats {
    fn record(&mut self, event: IndexTraversalEvent) {
        match event {
            IndexTraversalEvent::BlockInitialized(position) => {
                self.block_initializations += 1;
                self.initialized_blocks.insert(position);
            }
            IndexTraversalEvent::EntryDecoded(position, index) => {
                self.entry_decodes += 1;
                self.decoded_entries.insert((position, index));
            }
        }
    }

    fn merge(&mut self, other: &Self) {
        self.block_initializations += other.block_initializations;
        self.initialized_blocks.extend(&other.initialized_blocks);
        self.entry_decodes += other.entry_decodes;
        self.decoded_entries.extend(&other.decoded_entries);
    }
}

#[derive(Debug)]
enum ReftableLogPositionCursor {
    DeferredIndexed {
        position: u64,
        order: ReftableLogOrder,
    },
    Indexed(Vec<ReftableIndexFrame>),
    Unindexed {
        position: u64,
        end: u64,
    },
}

#[derive(Debug)]
struct ReftableIndexFrame {
    position: u64,
    next_index: usize,
    target_count: Option<usize>,
    next_entry_offset: u64,
    record_end: u64,
    previous_key_len: usize,
}

#[derive(Debug)]
struct ReftableLogSummaryBlock {
    records: Vec<ReftableLogItem>,
    next_position: u64,
    finished: bool,
}

impl<'a> ReftableLogCursor<'a> {
    pub(crate) fn new_oldest_to_newest(
        snapshot: &'a ReftableStackSnapshot,
        ref_name: &str,
    ) -> io::Result<Self> {
        Self::new_with_order(snapshot, ref_name, ReftableLogOrder::OldestToNewest, false)
    }

    pub(crate) fn new_newest_to_oldest(
        snapshot: &'a ReftableStackSnapshot,
        ref_name: &str,
    ) -> io::Result<Self> {
        Self::new_with_order(snapshot, ref_name, ReftableLogOrder::NewestToOldest, false)
    }

    pub(crate) fn new_newest_to_oldest_including_existence_markers(
        snapshot: &'a ReftableStackSnapshot,
        ref_name: &str,
    ) -> io::Result<Self> {
        Self::new_with_order(snapshot, ref_name, ReftableLogOrder::NewestToOldest, true)
    }

    fn new_with_order(
        snapshot: &'a ReftableStackSnapshot,
        ref_name: &str,
        order: ReftableLogOrder,
        include_existence_markers: bool,
    ) -> io::Result<Self> {
        if order == ReftableLogOrder::OldestToNewest {
            for table in &snapshot.tables {
                let _ = table.log_cursor(ref_name, order, include_existence_markers)?;
            }
        }
        let mut tables = Vec::new();
        match order {
            ReftableLogOrder::OldestToNewest => {
                for table in &snapshot.tables {
                    if let Some(cursor) =
                        table.log_cursor(ref_name, order, include_existence_markers)?
                    {
                        tables.push(cursor);
                    }
                }
            }
            ReftableLogOrder::NewestToOldest => {
                for table in snapshot.tables.iter().rev() {
                    if let Some(cursor) =
                        table.log_cursor(ref_name, order, include_existence_markers)?
                    {
                        tables.push(cursor);
                    }
                }
            }
        }
        let cursor = Self {
            order,
            heads: (0..tables.len()).map(|_| None).collect(),
            tables,
            heap: BinaryHeap::new(),
            heap_initialized: false,
            terminal_error: None,
        };
        Ok(cursor)
    }

    pub(crate) fn next(&mut self) -> io::Result<Option<ReftableLogSummary>> {
        if let Some(error) = &self.terminal_error {
            return Err(error.into_io_error());
        }
        match self.next_unpoisoned() {
            Ok(result) => Ok(result),
            Err(error) => {
                let terminal = ReftableLogTerminalError::from_error(&error);
                self.terminal_error = Some(terminal.clone());
                Err(terminal.into_io_error())
            }
        }
    }

    fn next_unpoisoned(&mut self) -> io::Result<Option<ReftableLogSummary>> {
        self.initialize_heap()?;
        loop {
            let Some(first) = self.heap.peek() else {
                return Ok(None);
            };
            let update_index = first.update_index;
            let mut selected_item = None;
            let mut grouped_tables = Vec::new();
            while self
                .heap
                .peek()
                .is_some_and(|entry| entry.update_index == update_index)
            {
                let entry = self.heap.pop().expect("heap head disappeared");
                if selected_item.is_none() {
                    selected_item = self.heads[entry.table_index].take();
                } else {
                    let _ = self.heads[entry.table_index]
                        .take()
                        .expect("grouped reftable log head disappeared");
                }
                grouped_tables.push(entry.table_index);
            }
            for table_index in grouped_tables {
                self.refill_head(table_index)?;
            }
            let selected_item = selected_item.expect("reftable log group was empty");
            if selected_item.is_tombstone() {
                continue;
            }
            let ReftableLogItem::Summary(summary) = selected_item else {
                unreachable!("tombstone handled above")
            };
            return Ok(Some(summary));
        }
    }

    fn initialize_heap(&mut self) -> io::Result<()> {
        if self.heap_initialized {
            return Ok(());
        }
        self.heap_initialized = true;
        for table_index in 0..self.tables.len() {
            self.refill_head(table_index)?;
        }
        Ok(())
    }

    fn refill_head(&mut self, table_index: usize) -> io::Result<()> {
        let item = self.tables[table_index].next_item()?;
        let Some(item) = item else {
            self.heads[table_index] = None;
            return Ok(());
        };
        let table_count = self.tables.len();
        let table_rank = match self.order {
            ReftableLogOrder::OldestToNewest => table_index,
            ReftableLogOrder::NewestToOldest => table_count - table_index,
        };
        self.heap.push(ReftableLogHeapEntry {
            order: self.order,
            table_index,
            table_rank,
            update_index: item.update_index(),
            tombstone: item.is_tombstone(),
        });
        self.heads[table_index] = Some(item);
        Ok(())
    }

    #[cfg(test)]
    fn index_traversal_stats(&self) -> IndexTraversalStats {
        let mut stats = IndexTraversalStats::default();
        for table in &self.tables {
            stats.merge(&table.index_stats);
        }
        stats
    }
}

pub(crate) fn open_stack(
    reftable_dir: &Path,
    algorithm: GitHashAlgorithm,
) -> io::Result<ReftableStackSnapshot> {
    for _ in 0..8 {
        let before = read_tables_list(reftable_dir)?;
        let names = parse_tables_list(&before)?;
        let mut opened = Vec::with_capacity(names.len());
        let mut retry = false;
        for name in &names {
            match ReftableTable::open(reftable_dir, name, algorithm) {
                Ok(table) => opened.push(table),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    retry = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        if retry {
            continue;
        }
        let after = read_tables_list(reftable_dir)?;
        if before == after {
            return Ok(ReftableStackSnapshot { tables: opened });
        }
    }
    Err(invalid("reftable tables.list changed during snapshot"))
}

impl ReftableTable {
    fn open(reftable_dir: &Path, name: &str, algorithm: GitHashAlgorithm) -> io::Result<Self> {
        let range = parse_table_name(name)?;
        let path = reftable_dir.join(name);
        let file = OpenOptions::new().read(true).open(path)?;
        let metadata = file.metadata()?;
        let len = metadata.len();
        let header = read_header(&file, len, algorithm)?;
        if header.min_update_index != range.min
            || header.max_update_index != range.max
            || header.min_update_index > header.max_update_index
        {
            return Err(invalid("reftable table name/header range mismatch"));
        }
        let footer = read_footer(&file, len, header, algorithm)?;
        let mut table = Self {
            name: name.to_owned(),
            file,
            len,
            identity: FileIdentity::from_metadata(&metadata),
            algorithm,
            header,
            footer,
        };
        table.normalize_log_position()?;
        table.validate_layout()?;
        Ok(table)
    }

    pub(crate) fn min_update_index(&self) -> u64 {
        self.header.min_update_index
    }

    pub(crate) fn max_update_index(&self) -> u64 {
        self.header.max_update_index
    }

    pub(crate) fn read_refs(&self) -> io::Result<Vec<(String, Option<ReftableParsedRefTarget>)>> {
        self.ensure_identity()?;
        if self.footer.start == self.header.len as u64 {
            return Ok(Vec::new());
        }
        if self.first_block_type()? != b'r' {
            return Ok(Vec::new());
        }
        let mut refs = Vec::new();
        self.for_each_fixed_block(
            FixedSection::Ref,
            0,
            self.next_boundary(0),
            true,
            |_, body| parse_ref_block(body, &self.header, self.algorithm, &mut refs),
        )?;
        Ok(refs)
    }

    pub(crate) fn read_logs(&self) -> io::Result<Vec<ReftableParsedLogRecord>> {
        self.ensure_identity()?;
        if self.footer.log_position == 0 {
            return Ok(Vec::new());
        }
        let mut logs = Vec::new();
        let _ = self.scan_logs(
            self.footer.log_position,
            self.next_boundary(self.footer.log_position),
            |_, _, body| parse_log_block(body, self.algorithm, &mut logs),
        )?;
        Ok(logs)
    }

    fn log_cursor<'a>(
        &'a self,
        ref_name: &str,
        order: ReftableLogOrder,
        include_existence_markers: bool,
    ) -> io::Result<Option<ReftableTableLogCursor<'a>>> {
        self.ensure_identity()?;
        if self.footer.log_position == 0 {
            return Ok(None);
        }
        let end = self.next_boundary(self.footer.log_position);
        let position_cursor = if self.footer.log_index_position != 0 {
            ReftableLogPositionCursor::DeferredIndexed {
                position: self.footer.log_index_position,
                order,
            }
        } else {
            if order == ReftableLogOrder::OldestToNewest {
                return Err(invalid(
                    "oldest-first reftable log traversal requires a log index",
                ));
            }
            ReftableLogPositionCursor::Unindexed {
                position: self.footer.log_position,
                end,
            }
        };
        Ok(Some(ReftableTableLogCursor {
            table: self,
            ref_name: ref_name.to_owned(),
            position_cursor,
            end,
            finished: false,
            pending: VecDeque::new(),
            order,
            include_existence_markers,
            #[cfg(test)]
            index_stats: IndexTraversalStats::default(),
        }))
    }

    fn ensure_identity(&self) -> io::Result<()> {
        let identity = FileIdentity::from_metadata(&self.file.metadata()?);
        if identity != self.identity || identity.len != self.len {
            return Err(invalid("reftable table changed after validation"));
        }
        Ok(())
    }

    fn normalize_log_position(&mut self) -> io::Result<()> {
        if self.footer.start == self.header.len as u64 {
            return Ok(());
        }
        if self.first_block_type()? == b'g' && self.footer.log_position == 0 {
            self.footer.log_position = self.header.len as u64;
        }
        Ok(())
    }

    fn validate_layout(&self) -> io::Result<()> {
        self.ensure_identity()?;
        if self.footer.start + self.footer_len() != self.len {
            return Err(invalid("reftable footer is not at end of file"));
        }
        if self.footer.start == self.header.len as u64 {
            if self.section_starts().iter().any(|position| *position != 0) {
                return Err(invalid("empty reftable has section offsets"));
            }
            return Ok(());
        }
        let first_type = self.first_block_type()?;
        let mut previous = 0;
        for position in self
            .section_starts()
            .into_iter()
            .filter(|value| *value != 0)
        {
            if position <= previous || position >= self.footer.start {
                return Err(invalid("invalid reftable section ordering"));
            }
            previous = position;
        }
        if first_type == b'r' {
            self.scan_fixed(FixedSection::Ref, 0, self.next_boundary(0), true)?;
        } else if first_type == b'g' {
            if self.footer.log_position != self.header.len as u64 {
                return Err(invalid("invalid first reftable log position"));
            }
        } else {
            return Err(invalid("unsupported first reftable block type"));
        }
        if self.footer.log_index_position != 0 && self.footer.log_position == 0 {
            return Err(invalid("log index without log section"));
        }
        if self.footer.ref_index_position != 0 {
            if first_type != b'r' {
                return Err(invalid("ref index without ref section"));
            }
            self.validate_index_root(
                self.footer.ref_index_position,
                self.footer.ref_index_position,
            )?;
            self.scan_fixed(
                FixedSection::Index,
                self.footer.ref_index_position,
                self.next_boundary(self.footer.ref_index_position),
                false,
            )?;
        }
        if self.footer.obj_position != 0 {
            if first_type != b'r' {
                return Err(invalid("object section without ref section"));
            }
            self.scan_fixed(
                FixedSection::Object,
                self.footer.obj_position,
                self.next_boundary(self.footer.obj_position),
                false,
            )?;
        }
        if self.footer.obj_index_position != 0 {
            self.validate_index_root(
                self.footer.obj_index_position,
                self.footer.obj_index_position,
            )?;
            self.scan_fixed(
                FixedSection::Index,
                self.footer.obj_index_position,
                self.next_boundary(self.footer.obj_index_position),
                false,
            )?;
        }
        if self.footer.log_position != 0 {
            let log_scan = self.scan_logs(
                self.footer.log_position,
                self.next_boundary(self.footer.log_position),
                |_, _, _| Ok(()),
            )?;
            match (
                self.footer.log_index_position,
                log_scan.first_index_position,
            ) {
                (0, Some(_)) => {
                    return Err(invalid("reftable log index has no footer root"));
                }
                (0, None) => {}
                (root, first) => {
                    self.validate_index_root(first.unwrap_or(root), root)?;
                    self.scan_fixed(FixedSection::Index, root, self.next_boundary(root), false)?;
                }
            }
        }
        Ok(())
    }

    fn section_starts(&self) -> [u64; 5] {
        [
            self.footer.ref_index_position,
            self.footer.obj_position,
            self.footer.obj_index_position,
            self.footer.log_position,
            self.footer.log_index_position,
        ]
    }

    fn next_boundary(&self, start: u64) -> u64 {
        self.section_starts()
            .into_iter()
            .filter(|candidate| *candidate > start)
            .min()
            .unwrap_or(self.footer.start)
    }

    fn first_block_type(&self) -> io::Result<u8> {
        if self.footer.start == self.header.len as u64 {
            return Err(invalid("reftable has no blocks"));
        }
        let mut block_type = [0_u8; 1];
        read_exact_at(&self.file, &mut block_type, self.header.len as u64)?;
        Ok(block_type[0])
    }

    fn footer_len(&self) -> u64 {
        match self.header.version {
            1 => FOOTER_V1_LEN as u64,
            2 => FOOTER_V2_LEN as u64,
            _ => unreachable!("validated reftable version"),
        }
    }

    fn scan_fixed(
        &self,
        section: FixedSection,
        start: u64,
        end: u64,
        first: bool,
    ) -> io::Result<()> {
        self.for_each_fixed_block(section, start, end, first, |_, _| Ok(()))
    }

    fn for_each_fixed_block(
        &self,
        section: FixedSection,
        start: u64,
        end: u64,
        first: bool,
        mut on_block: impl FnMut(u64, &[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        if start >= end || end > self.footer.start {
            return Err(invalid("invalid reftable fixed section bounds"));
        }
        let expected = match section {
            FixedSection::Ref => b'r',
            FixedSection::Index => b'i',
            FixedSection::Object => b'o',
        };
        let mut position = start;
        let mut first_block = first;
        loop {
            let header_offset = if first_block { self.header.len } else { 0 };
            let mut block_header = [0_u8; BLOCK_HEADER_LEN];
            read_exact_at(
                &self.file,
                &mut block_header,
                position + header_offset as u64,
            )?;
            if block_header[0] != expected {
                if matches!(section, FixedSection::Ref | FixedSection::Object)
                    && block_header[0] == b'i'
                {
                    let root = match section {
                        FixedSection::Ref => self.footer.ref_index_position,
                        FixedSection::Object => self.footer.obj_index_position,
                        FixedSection::Index => unreachable!(),
                    };
                    if root == 0 {
                        return Err(invalid("reftable index block without index section"));
                    }
                    self.validate_index_root(position, root)?;
                    self.scan_fixed(
                        FixedSection::Index,
                        position,
                        self.next_boundary(root),
                        false,
                    )?;
                    return Ok(());
                }
                return Err(invalid("unexpected reftable block type"));
            }
            let block_len = read_u24(&block_header[1..])?;
            let minimum = header_offset + BLOCK_HEADER_LEN;
            if block_len <= minimum || block_len > MAX_BLOCK_LEN {
                return Err(invalid("invalid reftable block length"));
            }
            if !matches!(section, FixedSection::Index)
                && self.header.block_size != 0
                && block_len > self.header.block_size
            {
                return Err(invalid("reftable block exceeds configured size"));
            }
            let block_end = if first_block {
                block_len as u64
            } else {
                position
                    .checked_add(block_len as u64)
                    .ok_or_else(|| invalid("reftable block offset overflow"))?
            };
            if block_end > end {
                return Err(invalid("reftable block crosses section boundary"));
            }
            let data_start = position
                .checked_add(header_offset as u64)
                .and_then(|value| value.checked_add(BLOCK_HEADER_LEN as u64))
                .ok_or_else(|| invalid("reftable block offset overflow"))?;
            let data_len = usize::try_from(
                block_end
                    .checked_sub(data_start)
                    .ok_or_else(|| invalid("reftable block data underflow"))?,
            )
            .map_err(|_| invalid("reftable block data length overflow"))?;
            let mut data = vec![0_u8; data_len];
            read_exact_at(&self.file, &mut data, data_start)?;
            on_block(position, &data)?;
            if block_end == end {
                return Ok(());
            }
            let next = if self.header.block_size == 0 {
                block_end
            } else if first_block {
                align_up(block_end, self.header.block_size as u64)?
            } else {
                position
                    .checked_add(self.header.block_size as u64)
                    .ok_or_else(|| invalid("reftable block boundary overflow"))?
            };
            if next < block_end || next > end {
                return Err(invalid("invalid reftable block boundary"));
            }
            validate_zero_padding(&self.file, block_end, next)?;
            if next == end {
                return Ok(());
            }
            position = next;
            first_block = false;
        }
    }

    fn scan_logs(
        &self,
        start: u64,
        end: u64,
        mut on_block: impl FnMut(u64, u64, &[u8]) -> io::Result<()>,
    ) -> io::Result<LogScanResult> {
        if start >= end || end > self.footer.start {
            return Err(invalid("invalid reftable log section bounds"));
        }
        let mut position = start;
        let mut first_block = start == self.header.len as u64;
        let mut saw_log_block = false;
        while position < end {
            let mut block_header = [0_u8; BLOCK_HEADER_LEN];
            read_exact_at(&self.file, &mut block_header, position)?;
            if block_header[0] == b'i' {
                if !saw_log_block {
                    return Err(invalid("reftable log index has no log blocks"));
                }
                return Ok(LogScanResult {
                    first_index_position: Some(position),
                });
            }
            if block_header[0] != b'g' {
                return Err(invalid("unexpected reftable log block type"));
            }
            let block_len = read_u24(&block_header[1..])?;
            let header_offset = if first_block { self.header.len } else { 0 };
            if block_len <= header_offset + BLOCK_HEADER_LEN || block_len > MAX_BLOCK_LEN {
                return Err(invalid("invalid reftable log block length"));
            }
            let body_len = block_len - header_offset - BLOCK_HEADER_LEN;
            let mut file = self.file.try_clone()?;
            file.seek(SeekFrom::Start(position + BLOCK_HEADER_LEN as u64))?;
            let remaining = end
                .checked_sub(position + BLOCK_HEADER_LEN as u64)
                .ok_or_else(|| invalid("reftable log block offset overflow"))?;
            let mut decoder = ZlibDecoder::new(file.take(remaining));
            let mut body = Vec::with_capacity(body_len);
            let mut buffer = [0_u8; IO_BUFFER_BYTES];
            loop {
                let count = decoder.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                if body.len() + count > body_len {
                    return Err(invalid("reftable log block exceeds declared size"));
                }
                body.extend_from_slice(&buffer[..count]);
            }
            if body.len() != body_len {
                return Err(invalid("reftable log block size mismatch"));
            }
            let consumed = decoder.total_in();
            if consumed == 0 {
                return Err(invalid("empty reftable log stream"));
            }
            let block_position = position;
            let next_position = position
                .checked_add(BLOCK_HEADER_LEN as u64)
                .and_then(|value| value.checked_add(consumed))
                .ok_or_else(|| invalid("reftable log block offset overflow"))?;
            if next_position > end {
                return Err(invalid("reftable log block crosses section boundary"));
            }
            on_block(block_position, next_position, &body)?;
            saw_log_block = true;
            position = next_position;
            first_block = false;
        }
        if position != end {
            return Err(invalid("reftable log section boundary mismatch"));
        }
        Ok(LogScanResult::default())
    }

    fn read_next_log_summary_block(
        &self,
        position: u64,
        end: u64,
        first_block: bool,
        saw_log_block: bool,
        ref_name: &str,
        include_existence_markers: bool,
    ) -> io::Result<ReftableLogSummaryBlock> {
        self.ensure_identity()?;
        if position >= end || end > self.footer.start {
            return Err(invalid("invalid reftable log section bounds"));
        }
        let mut block_header = [0_u8; BLOCK_HEADER_LEN];
        read_exact_at(&self.file, &mut block_header, position)?;
        if block_header[0] == b'i' {
            if !saw_log_block {
                return Err(invalid("reftable log index has no log blocks"));
            }
            return Ok(ReftableLogSummaryBlock {
                records: Vec::new(),
                next_position: end,
                finished: true,
            });
        }
        if block_header[0] != b'g' {
            return Err(invalid("unexpected reftable log block type"));
        }
        let block_len = read_u24(&block_header[1..])?;
        let header_offset = if first_block { self.header.len } else { 0 };
        if block_len <= header_offset + BLOCK_HEADER_LEN || block_len > MAX_BLOCK_LEN {
            return Err(invalid("invalid reftable log block length"));
        }
        let body_len = block_len - header_offset - BLOCK_HEADER_LEN;
        let mut file = self.file.try_clone()?;
        file.seek(SeekFrom::Start(position + BLOCK_HEADER_LEN as u64))?;
        let remaining = end
            .checked_sub(position + BLOCK_HEADER_LEN as u64)
            .ok_or_else(|| invalid("reftable log block offset overflow"))?;
        let mut decoder = ZlibDecoder::new(file.take(remaining));
        let mut body = Vec::with_capacity(body_len);
        let mut buffer = [0_u8; IO_BUFFER_BYTES];
        loop {
            let count = decoder.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            if body.len() + count > body_len {
                return Err(invalid("reftable log block exceeds declared size"));
            }
            body.extend_from_slice(&buffer[..count]);
        }
        if body.len() != body_len {
            return Err(invalid("reftable log block size mismatch"));
        }
        let mut records = Vec::new();
        parse_log_summary_block(
            &body,
            self.algorithm,
            ref_name.as_bytes(),
            include_existence_markers,
            |record| {
                records.push(record);
                Ok(())
            },
        )?;
        let consumed = decoder.total_in();
        if consumed == 0 {
            return Err(invalid("empty reftable log stream"));
        }
        let next_position = position
            .checked_add(BLOCK_HEADER_LEN as u64)
            .and_then(|value| value.checked_add(consumed))
            .ok_or_else(|| invalid("reftable log block offset overflow"))?;
        if next_position > end {
            return Err(invalid("reftable log block crosses section boundary"));
        }
        Ok(ReftableLogSummaryBlock {
            records,
            next_position,
            finished: false,
        })
    }

    fn new_index_frame(
        &self,
        position: u64,
        order: ReftableLogOrder,
        on_event: &mut impl FnMut(IndexTraversalEvent),
    ) -> io::Result<ReftableIndexFrame> {
        let target_count = if order == ReftableLogOrder::OldestToNewest {
            let body = self.read_index_block_body(position)?;
            Some(parse_index_block_target(&body, None)?.0)
        } else {
            None
        };
        let (data_start, record_end) = self.read_index_block_bounds(position)?;
        on_event(IndexTraversalEvent::BlockInitialized(position));
        Ok(ReftableIndexFrame {
            position,
            next_index: 0,
            target_count,
            next_entry_offset: data_start,
            record_end,
            previous_key_len: 0,
        })
    }

    fn read_index_block_target(
        &self,
        position: u64,
        target_index: usize,
        on_event: &mut impl FnMut(IndexTraversalEvent),
    ) -> io::Result<u64> {
        let body = self.read_index_block_body(position)?;
        let (_, target) = parse_index_block_target(&body, Some(target_index))?;
        on_event(IndexTraversalEvent::EntryDecoded(position, target_index));
        target.ok_or_else(|| invalid("reftable log index target is out of bounds"))
    }

    fn read_index_block_body(&self, position: u64) -> io::Result<Vec<u8>> {
        if position < self.header.len as u64 || position >= self.footer.start {
            return Err(invalid("reftable index block position out of bounds"));
        }
        let mut block_header = [0_u8; BLOCK_HEADER_LEN];
        read_exact_at(&self.file, &mut block_header, position)?;
        if block_header[0] != b'i' {
            return Err(invalid("reftable log index target is not an index block"));
        }
        let block_len = read_u24(&block_header[1..])?;
        if block_len <= BLOCK_HEADER_LEN || block_len > MAX_BLOCK_LEN {
            return Err(invalid("invalid reftable index block length"));
        }
        let block_end = position
            .checked_add(block_len as u64)
            .ok_or_else(|| invalid("reftable index block offset overflow"))?;
        let end = self.next_boundary(self.footer.log_index_position);
        if block_end > end {
            return Err(invalid("reftable index block crosses section boundary"));
        }
        let mut body = vec![0_u8; block_len - BLOCK_HEADER_LEN];
        read_exact_at(&self.file, &mut body, position + BLOCK_HEADER_LEN as u64)?;
        Ok(body)
    }

    fn read_index_block_bounds(&self, position: u64) -> io::Result<(u64, u64)> {
        if position < self.header.len as u64 || position >= self.footer.start {
            return Err(invalid("reftable index block position out of bounds"));
        }
        let mut block_header = [0_u8; BLOCK_HEADER_LEN];
        read_exact_at(&self.file, &mut block_header, position)?;
        if block_header[0] != b'i' {
            return Err(invalid("reftable log index target is not an index block"));
        }
        let block_len = read_u24(&block_header[1..])? as u64;
        if block_len <= BLOCK_HEADER_LEN as u64 || block_len > MAX_BLOCK_LEN as u64 {
            return Err(invalid("invalid reftable index block length"));
        }
        let block_end = position
            .checked_add(block_len)
            .ok_or_else(|| invalid("reftable index block offset overflow"))?;
        let end = self.next_boundary(self.footer.log_index_position);
        if block_end > end {
            return Err(invalid("reftable index block crosses section boundary"));
        }
        let mut restart_count_bytes = [0_u8; 2];
        read_exact_at(&self.file, &mut restart_count_bytes, block_end - 2)?;
        let restart_count = u64::from(u16::from_be_bytes(restart_count_bytes));
        let restart_bytes = restart_count
            .checked_mul(3)
            .and_then(|value| value.checked_add(2))
            .ok_or_else(|| invalid("reftable restart table overflow"))?;
        let record_end = block_end
            .checked_sub(restart_bytes)
            .ok_or_else(|| invalid("truncated reftable index restart table"))?;
        let data_start = position
            .checked_add(BLOCK_HEADER_LEN as u64)
            .ok_or_else(|| invalid("reftable index block offset overflow"))?;
        if record_end < data_start {
            return Err(invalid("truncated reftable index restart table"));
        }
        Ok((data_start, record_end))
    }

    fn read_index_varint(&self, position: &mut u64, end: u64) -> io::Result<u64> {
        if *position >= end {
            return Err(invalid("truncated reftable index varint"));
        }
        let mut byte = [0_u8; 1];
        read_exact_at(&self.file, &mut byte, *position)?;
        *position += 1;
        let mut value = u64::from(byte[0] & 0x7f);
        while byte[0] & 0x80 != 0 {
            if *position >= end {
                return Err(invalid("truncated reftable index varint"));
            }
            read_exact_at(&self.file, &mut byte, *position)?;
            *position += 1;
            value = value
                .checked_add(1)
                .and_then(|value| value.checked_shl(7))
                .map(|value| value | u64::from(byte[0] & 0x7f))
                .ok_or_else(|| invalid("reftable index varint overflow"))?;
        }
        Ok(value)
    }

    fn read_index_entry(
        &self,
        frame: &mut ReftableIndexFrame,
        on_event: &mut impl FnMut(IndexTraversalEvent),
    ) -> io::Result<u64> {
        if frame
            .target_count
            .is_some_and(|target_count| frame.next_index >= target_count)
            || frame.next_entry_offset >= frame.record_end
        {
            return Err(invalid("reftable log index target is out of bounds"));
        }
        let entry_index = frame.next_index;
        let mut position = frame.next_entry_offset;
        let prefix_len = usize::try_from(self.read_index_varint(&mut position, frame.record_end)?)
            .map_err(|_| invalid("reftable index prefix length overflow"))?;
        let suffix_and_type = self.read_index_varint(&mut position, frame.record_end)?;
        let suffix_len = usize::try_from(suffix_and_type >> 3)
            .map_err(|_| invalid("reftable index suffix length overflow"))?;
        if suffix_and_type & 0x7 != 0 || prefix_len > frame.previous_key_len {
            return Err(invalid(&format!(
                "invalid reftable index record prefix={prefix_len} previous={} type={}",
                frame.previous_key_len,
                suffix_and_type & 0x7
            )));
        }
        let suffix_end = position
            .checked_add(suffix_len as u64)
            .ok_or_else(|| invalid("reftable index suffix offset overflow"))?;
        if suffix_end > frame.record_end {
            return Err(invalid("invalid reftable index record"));
        }
        position = suffix_end;
        let value = self.read_index_varint(&mut position, frame.record_end)?;
        frame.previous_key_len = prefix_len + suffix_len;
        frame.next_entry_offset = position;
        frame.next_index += 1;
        on_event(IndexTraversalEvent::EntryDecoded(
            frame.position,
            entry_index,
        ));
        Ok(value)
    }

    fn log_block_type(&self, position: u64) -> io::Result<u8> {
        let position = if position == 0 {
            self.header.len as u64
        } else {
            position
        };
        if position < self.header.len as u64 || position >= self.footer.start {
            return Err(invalid("reftable log block position out of bounds"));
        }
        let mut block_type = [0_u8; 1];
        read_exact_at(&self.file, &mut block_type, position)?;
        Ok(block_type[0])
    }

    fn validate_index_root(&self, first: u64, root: u64) -> io::Result<()> {
        // Multi-level tables place lower index levels before the footer root;
        // the footer must name the first unreferenced (highest) level.
        if root == 0 || first > root {
            return Err(invalid("reftable index root precedes index blocks"));
        }
        let end = self.next_boundary(root);
        let mut blocks = Vec::new();
        self.for_each_fixed_block(FixedSection::Index, first, end, false, |position, body| {
            blocks.push(IndexBlock {
                position,
                targets: parse_index_block_targets(body)?,
            });
            Ok(())
        })?;
        if blocks.is_empty() {
            return Err(invalid("reftable index section is empty"));
        }

        let positions = blocks
            .iter()
            .map(|block| block.position)
            .collect::<HashSet<_>>();
        let mut referenced = HashSet::new();
        for block in &blocks {
            for target in &block.targets {
                if *target >= block.position {
                    return Err(invalid("reftable index points forward"));
                }
                if positions.contains(target) {
                    referenced.insert(*target);
                }
            }
        }
        let roots = blocks
            .iter()
            .filter(|block| !referenced.contains(&block.position))
            .collect::<Vec<_>>();
        if roots.len() != 1 || roots[0].position != root {
            return Err(invalid(
                "reftable footer index root is not the section root",
            ));
        }
        Ok(())
    }
}

impl<'a> ReftableTableLogCursor<'a> {
    fn next_item(&mut self) -> io::Result<Option<ReftableLogItem>> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Ok(Some(item));
            }
            if self.finished {
                return Ok(None);
            }
            #[cfg(test)]
            let next_position = self.position_cursor.next_position(
                self.table,
                self.order,
                &mut self.index_stats,
            )?;
            #[cfg(not(test))]
            let next_position = self.position_cursor.next_position(self.table, self.order)?;
            let Some(position) = next_position else {
                self.finished = true;
                return Ok(None);
            };
            let block = self.table.read_next_log_summary_block(
                position,
                self.end,
                position == self.table.header.len as u64,
                true,
                &self.ref_name,
                self.include_existence_markers,
            )?;
            if block.finished {
                self.finished = true;
                return Ok(None);
            }
            self.position_cursor.advance(position, block.next_position);
            let mut records = block.records;
            match self.order {
                ReftableLogOrder::OldestToNewest => {
                    records.sort_unstable_by_key(ReftableLogItem::update_index)
                }
                ReftableLogOrder::NewestToOldest => {
                    records.sort_unstable_by_key(|record| std::cmp::Reverse(record.update_index()))
                }
            }
            self.pending = records.into();
        }
    }
}

impl ReftableLogPositionCursor {
    #[cfg(not(test))]
    fn next_position(
        &mut self,
        table: &ReftableTable,
        order: ReftableLogOrder,
    ) -> io::Result<Option<u64>> {
        self.next_position_impl(table, order, &mut |_| {})
    }

    #[cfg(test)]
    fn next_position(
        &mut self,
        table: &ReftableTable,
        order: ReftableLogOrder,
        stats: &mut IndexTraversalStats,
    ) -> io::Result<Option<u64>> {
        self.next_position_impl(table, order, &mut |event| stats.record(event))
    }

    fn next_position_impl(
        &mut self,
        table: &ReftableTable,
        order: ReftableLogOrder,
        on_event: &mut impl FnMut(IndexTraversalEvent),
    ) -> io::Result<Option<u64>> {
        let deferred = match self {
            Self::DeferredIndexed { position, order } => Some((*position, *order)),
            _ => None,
        };
        if let Some((position, order)) = deferred {
            let frame = table.new_index_frame(position, order, on_event)?;
            *self = Self::Indexed(vec![frame]);
        }
        match self {
            Self::Indexed(frames) => loop {
                let Some(frame) = frames.last_mut() else {
                    return Ok(None);
                };
                let exhausted = match order {
                    ReftableLogOrder::OldestToNewest => frame
                        .target_count
                        .is_some_and(|target_count| frame.next_index >= target_count),
                    ReftableLogOrder::NewestToOldest => frame.next_entry_offset >= frame.record_end,
                };
                if exhausted {
                    frames.pop();
                    continue;
                }
                let target = match order {
                    ReftableLogOrder::OldestToNewest => {
                        let target_count = frame
                            .target_count
                            .expect("oldest-first index frame has no target count");
                        let target_index = target_count - 1 - frame.next_index;
                        frame.next_index += 1;
                        table.read_index_block_target(frame.position, target_index, on_event)?
                    }
                    ReftableLogOrder::NewestToOldest => table.read_index_entry(frame, on_event)?,
                };
                match table.log_block_type(target)? {
                    b'g' => {
                        return Ok(Some(if target == 0 {
                            table.header.len as u64
                        } else {
                            target
                        }));
                    }
                    b'i' => {
                        frames.push(table.new_index_frame(target, order, on_event)?);
                    }
                    _ => return Err(invalid("reftable log index points to non-log block")),
                }
            },
            Self::Unindexed { position, end } => {
                if *position >= *end {
                    return Ok(None);
                }
                Ok(Some(*position))
            }
            Self::DeferredIndexed { .. } => unreachable!("deferred index was initialized"),
        }
    }

    fn advance(&mut self, position: u64, next_position: u64) {
        if let Self::Unindexed {
            position: current, ..
        } = self
        {
            debug_assert_eq!(*current, position);
            *current = next_position;
        }
    }
}

fn parse_ref_block(
    body: &[u8],
    header: &Header,
    algorithm: GitHashAlgorithm,
    refs: &mut Vec<(String, Option<ReftableParsedRefTarget>)>,
) -> io::Result<()> {
    let record_end = restart_record_end(body, "reftable ref")?;
    let mut cursor = 0;
    let mut prior_name = Vec::new();
    while cursor < record_end {
        let prefix_len = usize::try_from(read_varint(body, &mut cursor, record_end)?)
            .map_err(|_| invalid("reftable ref prefix length overflow"))?;
        let suffix_and_type = read_varint(body, &mut cursor, record_end)?;
        let suffix_len = usize::try_from(suffix_and_type >> 3)
            .map_err(|_| invalid("reftable ref suffix length overflow"))?;
        let value_type = (suffix_and_type & 0x7) as u8;
        if prefix_len > prior_name.len() || suffix_len > record_end.saturating_sub(cursor) {
            return Err(invalid("invalid reftable ref name"));
        }
        let mut name = prior_name[..prefix_len].to_vec();
        name.extend_from_slice(&body[cursor..cursor + suffix_len]);
        cursor += suffix_len;
        let _update_index = header
            .min_update_index
            .checked_add(read_varint(body, &mut cursor, record_end)?)
            .ok_or_else(|| invalid("reftable update index overflow"))?;
        let name = String::from_utf8(name).map_err(|_| invalid("non-utf8 ref name"))?;
        let target = match value_type {
            0 => None,
            1 | 2 => {
                let id = read_object_id(body, &mut cursor, record_end, algorithm)?;
                if value_type == 2 {
                    let _peeled = read_object_id(body, &mut cursor, record_end, algorithm)?;
                }
                Some(ReftableParsedRefTarget::Direct(id))
            }
            3 => Some(ReftableParsedRefTarget::Symbolic(read_string(
                body,
                &mut cursor,
                record_end,
                "symbolic reftable ref",
            )?)),
            _ => return Err(invalid("unsupported reftable ref value type")),
        };
        prior_name = name.as_bytes().to_vec();
        if crate::refs::validate_storable_ref_name(&name).is_ok() {
            refs.push((name, target));
        }
    }
    Ok(())
}

fn parse_index_block_targets(body: &[u8]) -> io::Result<Vec<u64>> {
    let record_end = restart_record_end(body, "reftable index")?;
    let mut cursor = 0;
    let mut prior_key = Vec::new();
    let mut targets = Vec::new();
    while cursor < record_end {
        let prefix_len = usize::try_from(read_varint(body, &mut cursor, record_end)?)
            .map_err(|_| invalid("reftable index prefix length overflow"))?;
        let suffix_and_type = read_varint(body, &mut cursor, record_end)?;
        let suffix_len = usize::try_from(suffix_and_type >> 3)
            .map_err(|_| invalid("reftable index suffix length overflow"))?;
        if suffix_and_type & 0x7 != 0
            || prefix_len > prior_key.len()
            || suffix_len > record_end.saturating_sub(cursor)
        {
            return Err(invalid("invalid reftable index record"));
        }
        let mut key = prior_key[..prefix_len].to_vec();
        key.extend_from_slice(&body[cursor..cursor + suffix_len]);
        cursor += suffix_len;
        targets.push(read_varint(body, &mut cursor, record_end)?);
        prior_key = key;
    }
    Ok(targets)
}

fn parse_index_block_target(
    body: &[u8],
    target_index: Option<usize>,
) -> io::Result<(usize, Option<u64>)> {
    let record_end = restart_record_end(body, "reftable index")?;
    let mut cursor = 0;
    let mut prior_key = Vec::new();
    let mut count = 0;
    let mut target = None;
    while cursor < record_end {
        let prefix_len = usize::try_from(read_varint(body, &mut cursor, record_end)?)
            .map_err(|_| invalid("reftable index prefix length overflow"))?;
        let suffix_and_type = read_varint(body, &mut cursor, record_end)?;
        let suffix_len = usize::try_from(suffix_and_type >> 3)
            .map_err(|_| invalid("reftable index suffix length overflow"))?;
        if suffix_and_type & 0x7 != 0
            || prefix_len > prior_key.len()
            || suffix_len > record_end.saturating_sub(cursor)
        {
            return Err(invalid("invalid reftable index record"));
        }
        let mut key = prior_key[..prefix_len].to_vec();
        key.extend_from_slice(&body[cursor..cursor + suffix_len]);
        cursor += suffix_len;
        let value = read_varint(body, &mut cursor, record_end)?;
        if target_index == Some(count) {
            target = Some(value);
        }
        count += 1;
        prior_key = key;
    }
    Ok((count, target))
}

fn parse_log_block(
    body: &[u8],
    algorithm: GitHashAlgorithm,
    logs: &mut Vec<ReftableParsedLogRecord>,
) -> io::Result<()> {
    parse_log_block_with_callback(body, algorithm, |record| {
        logs.push(record);
        Ok(())
    })
}

fn parse_log_block_with_callback(
    body: &[u8],
    algorithm: GitHashAlgorithm,
    mut on_record: impl FnMut(ReftableParsedLogRecord) -> io::Result<()>,
) -> io::Result<()> {
    let record_end = restart_record_end(body, "reftable log")?;
    let mut cursor = 0;
    let mut prior_key = Vec::new();
    while cursor < record_end {
        let prefix_len = usize::try_from(read_varint(body, &mut cursor, record_end)?)
            .map_err(|_| invalid("reftable log prefix length overflow"))?;
        let suffix_and_type = read_varint(body, &mut cursor, record_end)?;
        let suffix_len = usize::try_from(suffix_and_type >> 3)
            .map_err(|_| invalid("reftable log suffix length overflow"))?;
        let value_type = (suffix_and_type & 0x7) as u8;
        if prefix_len > prior_key.len() || suffix_len > record_end.saturating_sub(cursor) {
            return Err(invalid("invalid reftable log key"));
        }
        let mut key = prior_key[..prefix_len].to_vec();
        key.extend_from_slice(&body[cursor..cursor + suffix_len]);
        cursor += suffix_len;
        let (ref_name, update_index) = parse_log_key(&key)?;
        let parsed = match value_type {
            0 => ReftableParsedLogRecord::Deletion {
                ref_name,
                update_index,
            },
            1 => {
                let old_id = read_object_id(body, &mut cursor, record_end, algorithm)?;
                let new_id = read_object_id(body, &mut cursor, record_end, algorithm)?;
                let name = read_string(body, &mut cursor, record_end, "reftable log name")?;
                let email = read_string(body, &mut cursor, record_end, "reftable log email")?;
                let timestamp = read_varint(body, &mut cursor, record_end)?;
                if cursor > record_end.saturating_sub(2) {
                    return Err(invalid("truncated reftable log timezone"));
                }
                let timezone_offset = i16::from_be_bytes([body[cursor], body[cursor + 1]]);
                cursor += 2;
                let message = read_string(body, &mut cursor, record_end, "reftable log message")?;
                ReftableParsedLogRecord::Update(ReftableLogRecord {
                    ref_name,
                    update_index,
                    old_id,
                    new_id,
                    name,
                    email,
                    timestamp,
                    timezone_offset,
                    message,
                })
            }
            _ => return Err(invalid("unsupported reftable log value type")),
        };
        prior_key = key;
        on_record(parsed)?;
    }
    Ok(())
}

fn parse_log_summary_block(
    body: &[u8],
    algorithm: GitHashAlgorithm,
    wanted_ref: &[u8],
    include_existence_markers: bool,
    mut on_record: impl FnMut(ReftableLogItem) -> io::Result<()>,
) -> io::Result<()> {
    let record_end = restart_record_end(body, "reftable log")?;
    let mut cursor = 0;
    let mut prior_key = Vec::new();
    while cursor < record_end {
        let prefix_len = usize::try_from(read_varint(body, &mut cursor, record_end)?)
            .map_err(|_| invalid("reftable log prefix length overflow"))?;
        let suffix_and_type = read_varint(body, &mut cursor, record_end)?;
        let suffix_len = usize::try_from(suffix_and_type >> 3)
            .map_err(|_| invalid("reftable log suffix length overflow"))?;
        let value_type = (suffix_and_type & 0x7) as u8;
        if prefix_len > prior_key.len() || suffix_len > record_end.saturating_sub(cursor) {
            return Err(invalid("invalid reftable log key"));
        }
        let mut key = prior_key[..prefix_len].to_vec();
        key.extend_from_slice(&body[cursor..cursor + suffix_len]);
        cursor += suffix_len;
        let (ref_name, update_index) = parse_log_key_bytes(&key)?;
        match value_type {
            0 => {
                if ref_name == wanted_ref {
                    on_record(ReftableLogItem::Tombstone { update_index })?;
                }
            }
            1 => {
                let old_id = read_object_id(body, &mut cursor, record_end, algorithm)?;
                let new_id = read_object_id(body, &mut cursor, record_end, algorithm)?;
                skip_reftable_string(body, &mut cursor, record_end, "reftable log name")?;
                skip_reftable_string(body, &mut cursor, record_end, "reftable log email")?;
                let timestamp = read_varint(body, &mut cursor, record_end)?;
                if cursor > record_end.saturating_sub(2) {
                    return Err(invalid("truncated reftable log timezone"));
                }
                let timezone_offset = i16::from_be_bytes([body[cursor], body[cursor + 1]]);
                cursor += 2;
                skip_reftable_string(body, &mut cursor, record_end, "reftable log message")?;
                let is_existence_marker = old_id.as_bytes().iter().all(|byte| *byte == 0)
                    && new_id.as_bytes().iter().all(|byte| *byte == 0);
                if ref_name == wanted_ref && (include_existence_markers || !is_existence_marker) {
                    on_record(ReftableLogItem::Summary(ReftableLogSummary {
                        update_index,
                        old_id,
                        new_id,
                        timestamp,
                        timezone_offset,
                    }))?;
                }
            }
            _ => return Err(invalid("unsupported reftable log value type")),
        }
        prior_key = key;
    }
    Ok(())
}

fn skip_reftable_string(
    bytes: &[u8],
    cursor: &mut usize,
    end: usize,
    label: &str,
) -> io::Result<()> {
    let length = usize::try_from(read_varint(bytes, cursor, end)?)
        .map_err(|_| invalid(&format!("{label} length overflow")))?;
    if length > end.saturating_sub(*cursor) {
        return Err(invalid(&format!("truncated {label}")));
    }
    std::str::from_utf8(&bytes[*cursor..*cursor + length])
        .map_err(|_| invalid(&format!("non-utf8 {label}")))?;
    *cursor += length;
    Ok(())
}

fn parse_log_key(key: &[u8]) -> io::Result<(String, u64)> {
    let (ref_name, update_index) = parse_log_key_bytes(key)?;
    let ref_name = String::from_utf8(ref_name.to_vec())
        .map_err(|_| invalid("non-utf8 reftable log ref name"))?;
    Ok((ref_name, update_index))
}

fn parse_log_key_bytes(key: &[u8]) -> io::Result<(&[u8], u64)> {
    if key.len() <= 9 || key[key.len() - 9] != 0 {
        return Err(invalid("invalid reftable log key"));
    }
    let name_end = key.len() - 9;
    let ref_name = std::str::from_utf8(&key[..name_end])
        .map_err(|_| invalid("non-utf8 reftable log ref name"))?;
    crate::refs::validate_storable_ref_name(ref_name)?;
    let encoded = u64::from_be_bytes(
        key[key.len() - 8..]
            .try_into()
            .expect("reftable log key length checked"),
    );
    Ok((&key[..name_end], u64::MAX - encoded))
}

fn restart_record_end(body: &[u8], section: &str) -> io::Result<usize> {
    let restart_count_offset = body
        .len()
        .checked_sub(2)
        .ok_or_else(|| invalid("truncated reftable restart count"))?;
    let restart_count =
        u16::from_be_bytes([body[restart_count_offset], body[restart_count_offset + 1]]) as usize;
    restart_count_offset
        .checked_sub(
            restart_count
                .checked_mul(3)
                .ok_or_else(|| invalid("reftable restart table overflow"))?,
        )
        .ok_or_else(|| invalid(&format!("truncated {section} restart table")))
}

fn read_varint(bytes: &[u8], cursor: &mut usize, end: usize) -> io::Result<u64> {
    if *cursor >= end {
        return Err(invalid("truncated reftable varint"));
    }
    let mut value = (bytes[*cursor] & 0x7f) as u64;
    while bytes[*cursor] & 0x80 != 0 {
        *cursor += 1;
        if *cursor >= end {
            return Err(invalid("truncated reftable varint"));
        }
        value = value
            .checked_add(1)
            .and_then(|value| value.checked_shl(7))
            .map(|value| value | u64::from(bytes[*cursor] & 0x7f))
            .ok_or_else(|| invalid("reftable varint overflow"))?;
    }
    *cursor += 1;
    Ok(value)
}

fn read_object_id(
    bytes: &[u8],
    cursor: &mut usize,
    end: usize,
    algorithm: GitHashAlgorithm,
) -> io::Result<ObjectId> {
    let length = match algorithm {
        GitHashAlgorithm::Sha1 => 20,
        GitHashAlgorithm::Sha256 => 32,
    };
    if length > end.saturating_sub(*cursor) {
        return Err(invalid("truncated reftable object id"));
    }
    let id = ObjectId::new(algorithm, &bytes[*cursor..*cursor + length]);
    *cursor += length;
    Ok(id)
}

fn read_string(bytes: &[u8], cursor: &mut usize, end: usize, label: &str) -> io::Result<String> {
    let length = usize::try_from(read_varint(bytes, cursor, end)?)
        .map_err(|_| invalid("reftable string length overflow"))?;
    if length > end.saturating_sub(*cursor) {
        return Err(invalid(&format!("truncated {label}")));
    }
    let value = String::from_utf8(bytes[*cursor..*cursor + length].to_vec())
        .map_err(|_| invalid("non-utf8 reftable string"))?;
    *cursor += length;
    Ok(value)
}

fn read_tables_list(reftable_dir: &Path) -> io::Result<Vec<u8>> {
    let path = reftable_dir.join("tables.list");
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    if len > MAX_TABLES_LIST_BYTES {
        return Err(invalid("reftable tables.list is too large"));
    }
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(MAX_TABLES_LIST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TABLES_LIST_BYTES {
        return Err(invalid("reftable tables.list is too large"));
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Copy)]
struct NameRange {
    min: u64,
    max: u64,
}

fn parse_tables_list(bytes: &[u8]) -> io::Result<Vec<String>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    let mut previous_max = None;
    for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        if line.is_empty() {
            if index + 1 == bytes.split(|byte| *byte == b'\n').count() && bytes.ends_with(b"\n") {
                continue;
            }
            return Err(invalid("empty reftable tables.list entry"));
        }
        if line.len() > MAX_TABLE_NAME_BYTES || line.contains(&b'\r') {
            return Err(invalid("invalid reftable tables.list entry"));
        }
        let name = std::str::from_utf8(line)
            .map_err(|_| invalid("non-utf8 reftable tables.list entry"))?;
        let range = parse_table_name(name)?;
        if let Some(previous_max) = previous_max
            && range.min <= previous_max
        {
            return Err(invalid("reftable tables.list is out of order"));
        }
        previous_max = Some(range.max);
        names.push(name.to_owned());
    }
    Ok(names)
}

fn parse_table_name(name: &str) -> io::Result<NameRange> {
    if name.is_empty()
        || name.len() > MAX_TABLE_NAME_BYTES
        || name
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte < 0x20)
        || name.contains('/')
        || name.contains('\\')
    {
        return Err(invalid("invalid reftable table name"));
    }
    let (stem, extension) = name
        .rsplit_once('.')
        .ok_or_else(|| invalid("invalid reftable table name"))?;
    if extension != "ref" && extension != "log" {
        return Err(invalid("invalid reftable table name"));
    }
    let mut parts = stem.split('-');
    let min = parse_hex_component(parts.next())?;
    let max = parse_hex_component(parts.next())?;
    let random = parts
        .next()
        .ok_or_else(|| invalid("invalid reftable table name"))?;
    if random.is_empty()
        || !random.bytes().all(|byte| byte.is_ascii_alphanumeric())
        || parts.next().is_some()
        || min > max
    {
        return Err(invalid("invalid reftable table name"));
    }
    Ok(NameRange { min, max })
}

fn parse_hex_component(value: Option<&str>) -> io::Result<u64> {
    let value = value.ok_or_else(|| invalid("invalid reftable table name"))?;
    let value = value.strip_prefix("0x").unwrap_or(value);
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("invalid reftable table name"));
    }
    u64::from_str_radix(value, 16).map_err(|_| invalid("reftable table update range overflow"))
}

fn read_header(file: &File, len: u64, algorithm: GitHashAlgorithm) -> io::Result<Header> {
    if len < HEADER_V1_LEN as u64 {
        return Err(invalid("truncated reftable header"));
    }
    let mut bytes = [0_u8; HEADER_V2_LEN];
    read_exact_at(file, &mut bytes[..HEADER_V1_LEN], 0)?;
    if &bytes[..4] != b"REFT" {
        return Err(invalid("invalid reftable header"));
    }
    let version = bytes[4];
    let header_len = match version {
        1 => {
            if algorithm != GitHashAlgorithm::Sha1 {
                return Err(invalid("reftable v1 requires sha1"));
            }
            HEADER_V1_LEN
        }
        2 => {
            if len < HEADER_V2_LEN as u64 {
                return Err(invalid("truncated reftable v2 header"));
            }
            read_exact_at(file, &mut bytes[24..], 24)?;
            validate_hash_id(&bytes[24..28], algorithm)?;
            HEADER_V2_LEN
        }
        _ => return Err(invalid("unsupported reftable version")),
    };
    let header = Header {
        version,
        len: header_len,
        block_size: read_u24(&bytes[5..8])?,
        min_update_index: read_u64(&bytes[8..16])?,
        max_update_index: read_u64(&bytes[16..24])?,
    };
    if header.min_update_index > header.max_update_index {
        return Err(invalid("invalid reftable update range"));
    }
    Ok(header)
}

fn read_footer(
    file: &File,
    len: u64,
    header: Header,
    algorithm: GitHashAlgorithm,
) -> io::Result<Footer> {
    let footer_len = match header.version {
        1 => FOOTER_V1_LEN,
        2 => FOOTER_V2_LEN,
        _ => unreachable!("validated reftable version"),
    };
    let start = len
        .checked_sub(footer_len as u64)
        .ok_or_else(|| invalid("truncated reftable footer"))?;
    if start < header.len as u64 {
        return Err(invalid("truncated reftable table"));
    }
    let mut bytes = vec![0_u8; footer_len];
    read_exact_at(file, &mut bytes, start)?;
    if bytes[..header.len] != read_header_bytes(file, header.len)?[..] {
        return Err(invalid("reftable footer/header mismatch"));
    }
    if header.version == 2 {
        validate_hash_id(&bytes[24..28], algorithm)?;
    }
    let mut hasher = Crc32Hasher::new();
    hasher.update(&bytes[..footer_len - 4]);
    if hasher.finalize() != read_u32(&bytes[footer_len - 4..])? {
        return Err(invalid("reftable footer checksum mismatch"));
    }
    let field = header.len;
    let ref_index_position = read_u64(&bytes[field..field + 8])?;
    let object_position = read_u64(&bytes[field + 8..field + 16])?;
    let encoded_obj_id_len = (object_position & 0x1f) as usize;
    let obj_position = object_position >> 5;
    let obj_index_position = read_u64(&bytes[field + 16..field + 24])?;
    let log_position = read_u64(&bytes[field + 24..field + 32])?;
    let log_index_position = read_u64(&bytes[field + 32..field + 40])?;
    for position in [
        ref_index_position,
        obj_position,
        obj_index_position,
        log_position,
        log_index_position,
    ] {
        if position != 0 && (position < header.len as u64 || position >= start) {
            return Err(invalid("reftable section offset is out of bounds"));
        }
    }
    let obj_id_len = if object_position == 0 {
        0
    } else {
        let max_id_len = match algorithm {
            GitHashAlgorithm::Sha1 => 20,
            GitHashAlgorithm::Sha256 => 32,
        };
        let obj_id_len = if encoded_obj_id_len == 0 {
            if algorithm == GitHashAlgorithm::Sha256 {
                max_id_len
            } else {
                return Err(invalid("invalid reftable object id length"));
            }
        } else {
            encoded_obj_id_len
        };
        if !(2..=max_id_len).contains(&obj_id_len) {
            return Err(invalid("invalid reftable object id length"));
        }
        obj_id_len
    };
    if obj_index_position != 0 && obj_position == 0 {
        return Err(invalid("object index without object section"));
    }
    Ok(Footer {
        ref_index_position,
        obj_position,
        obj_id_len,
        obj_index_position,
        log_position,
        log_index_position,
        start,
    })
}

fn validate_hash_id(bytes: &[u8], algorithm: GitHashAlgorithm) -> io::Result<()> {
    let expected = match algorithm {
        GitHashAlgorithm::Sha1 => b"sha1".as_slice(),
        GitHashAlgorithm::Sha256 => b"s256".as_slice(),
    };
    if bytes != expected {
        return Err(invalid("reftable hash id mismatch"));
    }
    Ok(())
}

fn read_header_bytes(file: &File, len: usize) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0_u8; len];
    read_exact_at(file, &mut bytes, 0)?;
    Ok(bytes)
}

fn read_exact_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<()> {
    let mut read = 0;
    while read < buffer.len() {
        let count = read_at_positional(file, &mut buffer[read..], offset + read as u64)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated reftable",
            ));
        }
        read += count;
    }
    Ok(())
}

#[cfg(unix)]
fn read_at_positional(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    file.read_at(buffer, offset)
}

#[cfg(windows)]
fn read_at_positional(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    let mut reopened = reopen_file_object(file)?;
    reopened.seek(SeekFrom::Start(offset))?;
    reopened.read(buffer)
}

#[cfg(not(any(unix, windows)))]
fn read_at_positional(_file: &File, _buffer: &mut [u8], _offset: u64) -> io::Result<usize> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "positional reftable reads are unavailable on this platform",
    ))
}

#[cfg(windows)]
fn reopen_file_object(file: &File) -> io::Result<File> {
    let handle = unsafe {
        ReOpenFile(
            file.as_raw_handle() as HANDLE,
            FILE_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle as RawHandle) })
}

fn validate_zero_padding(file: &File, start: u64, end: u64) -> io::Result<()> {
    let mut position = start;
    let mut bytes = [0_u8; IO_BUFFER_BYTES];
    while position < end {
        let count = usize::try_from((end - position).min(bytes.len() as u64))
            .map_err(|_| invalid("reftable padding length overflow"))?;
        read_exact_at(file, &mut bytes[..count], position)?;
        if bytes[..count].iter().any(|byte| *byte != 0) {
            return Err(invalid("non-zero reftable block padding"));
        }
        position += count as u64;
    }
    Ok(())
}

fn align_up(value: u64, alignment: u64) -> io::Result<u64> {
    if alignment == 0 {
        return Ok(value);
    }
    let remainder = value % alignment;
    if remainder == 0 {
        Ok(value)
    } else {
        value
            .checked_add(alignment - remainder)
            .ok_or_else(|| invalid("reftable block alignment overflow"))
    }
}

fn read_u24(bytes: &[u8]) -> io::Result<usize> {
    if bytes.len() < 3 {
        return Err(invalid("truncated reftable uint24"));
    }
    Ok(((bytes[0] as usize) << 16) | ((bytes[1] as usize) << 8) | bytes[2] as usize)
}

fn read_u32(bytes: &[u8]) -> io::Result<u32> {
    if bytes.len() < 4 {
        return Err(invalid("truncated reftable uint32"));
    }
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64(bytes: &[u8]) -> io::Result<u64> {
    if bytes.len() < 8 {
        return Err(invalid("truncated reftable uint64"));
    }
    Ok(u64::from_be_bytes(
        bytes[..8].try_into().expect("slice length checked"),
    ))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftable_writer::{ReftableEncodedRecord, ReftableWriteOptions, encode_reftable};
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;

    const SHA1_TABLE: &str = "0x000000000001-0x000000000001-test.ref";
    const MULTI_LEVEL_LOG_TABLE: &str = "0x000000000001-0x000000000400-multilog.ref";
    const SHA256_TABLE: &str = "0x000000000001-0x000000000001-test.ref";

    fn empty_table(algorithm: GitHashAlgorithm, min: u64, max: u64) -> Vec<u8> {
        encode_reftable(
            algorithm,
            min,
            max,
            &[],
            &[],
            ReftableWriteOptions::default(),
        )
        .expect("encode empty reftable")
    }

    fn table_with_refs(algorithm: GitHashAlgorithm, min: u64, max: u64, count: usize) -> Vec<u8> {
        let object_len = match algorithm {
            GitHashAlgorithm::Sha1 => 20,
            GitHashAlgorithm::Sha256 => 32,
        };
        let records = (0..count)
            .map(|index| {
                let mut value = vec![0];
                value.extend(std::iter::repeat_n(index as u8, object_len));
                ReftableEncodedRecord {
                    key: format!("refs/heads/branch-{index:03}").into_bytes(),
                    value_type: 1,
                    value,
                    object_ids: Vec::new(),
                }
            })
            .collect::<Vec<_>>();
        encode_reftable(
            algorithm,
            min,
            max,
            &records,
            &[],
            ReftableWriteOptions {
                block_size: 128,
                restart_interval: 2,
                index_objects: false,
            },
        )
        .expect("encode reftable refs")
    }

    fn table_with_sha256_objects() -> Vec<u8> {
        let records = (0..64)
            .map(|index| {
                let mut id = vec![0x11; 32];
                id[31] = index as u8;
                let mut value = vec![0];
                value.extend_from_slice(&id);
                ReftableEncodedRecord {
                    key: format!("refs/heads/object-{index:03}").into_bytes(),
                    value_type: 1,
                    value,
                    object_ids: vec![id],
                }
            })
            .collect::<Vec<_>>();
        encode_reftable(
            GitHashAlgorithm::Sha256,
            1,
            1,
            &records,
            &[],
            ReftableWriteOptions {
                block_size: 128,
                restart_interval: 2,
                index_objects: true,
            },
        )
        .expect("encode sha256 object reftable")
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

    fn log_table_for_records(
        algorithm: GitHashAlgorithm,
        min_index: u64,
        max_index: u64,
        records: &[(&str, u64, u8)],
    ) -> Vec<u8> {
        encode_log_table_for_records(algorithm, min_index, max_index, records)
    }

    fn encode_log_table_for_records(
        algorithm: GitHashAlgorithm,
        min_index: u64,
        max_index: u64,
        records: &[(&str, u64, u8)],
    ) -> Vec<u8> {
        let object_len = match algorithm {
            GitHashAlgorithm::Sha1 => 20,
            GitHashAlgorithm::Sha256 => 32,
        };
        let mut encoded = records
            .iter()
            .map(|(ref_name, update_index, seed)| {
                let mut key = ref_name.as_bytes().to_vec();
                key.push(0);
                key.extend_from_slice(&(u64::MAX - update_index).to_be_bytes());
                let mut value = vec![*seed; object_len * 2];
                write_varint(&mut value, 4);
                value.extend_from_slice(b"name");
                write_varint(&mut value, 5);
                value.extend_from_slice(b"email");
                write_varint(&mut value, 1);
                value.extend_from_slice(&0_i16.to_be_bytes());
                write_varint(&mut value, 8);
                value.extend_from_slice(b"message\n");
                ReftableEncodedRecord {
                    key,
                    value_type: 1,
                    value,
                    object_ids: Vec::new(),
                }
            })
            .collect::<Vec<_>>();
        encoded.sort_by(|left, right| left.key.cmp(&right.key));
        encode_reftable(
            algorithm,
            min_index,
            max_index,
            &[],
            &encoded,
            ReftableWriteOptions::default(),
        )
        .expect("encode log-only reftable")
    }

    fn log_table_for_index(algorithm: GitHashAlgorithm, update_index: u64) -> Vec<u8> {
        log_table_for_records(
            algorithm,
            update_index,
            update_index,
            &[("refs/heads/log-only", update_index, 0x22)],
        )
    }

    fn deletion_table_for_indices(
        algorithm: GitHashAlgorithm,
        header_index: u64,
        key_index: u64,
    ) -> Vec<u8> {
        deletion_table_for_records(
            algorithm,
            header_index,
            header_index,
            &[("refs/heads/log-only", key_index)],
        )
    }

    fn deletion_table_for_records(
        algorithm: GitHashAlgorithm,
        min_index: u64,
        max_index: u64,
        records: &[(&str, u64)],
    ) -> Vec<u8> {
        encode_deletion_table_for_records(algorithm, min_index, max_index, records)
    }

    fn encode_deletion_table_for_records(
        algorithm: GitHashAlgorithm,
        min_index: u64,
        max_index: u64,
        records: &[(&str, u64)],
    ) -> Vec<u8> {
        let mut encoded = records
            .iter()
            .map(|(ref_name, update_index)| {
                let mut key = ref_name.as_bytes().to_vec();
                key.push(0);
                key.extend_from_slice(&(u64::MAX - update_index).to_be_bytes());
                ReftableEncodedRecord {
                    key,
                    value_type: 0,
                    value: Vec::new(),
                    object_ids: Vec::new(),
                }
            })
            .collect::<Vec<_>>();
        encoded.sort_by(|left, right| left.key.cmp(&right.key));
        encode_reftable(
            algorithm,
            min_index,
            max_index,
            &[],
            &encoded,
            ReftableWriteOptions::default(),
        )
        .expect("encode log tombstone reftable")
    }

    fn indexed_interleaved_records(
        ref_name: &'static str,
        first_index: u64,
        seed: u8,
    ) -> Vec<(&'static str, u64, u8)> {
        const UNRELATED_REFS: [&str; 22] = [
            "refs/heads/unrelated-a",
            "refs/heads/unrelated-b",
            "refs/heads/unrelated-c",
            "refs/heads/unrelated-d",
            "refs/heads/unrelated-e",
            "refs/heads/unrelated-f",
            "refs/heads/unrelated-g",
            "refs/heads/unrelated-h",
            "refs/heads/unrelated-i",
            "refs/heads/unrelated-j",
            "refs/heads/unrelated-k",
            "refs/heads/unrelated-l",
            "refs/heads/unrelated-m",
            "refs/heads/unrelated-n",
            "refs/heads/unrelated-o",
            "refs/heads/unrelated-p",
            "refs/heads/unrelated-q",
            "refs/heads/unrelated-r",
            "refs/heads/unrelated-s",
            "refs/heads/unrelated-t",
            "refs/heads/unrelated-u",
            "refs/heads/unrelated-v",
        ];
        let mut records = vec![
            (ref_name, first_index, seed),
            (ref_name, first_index + 1, seed.wrapping_add(1)),
        ];
        records.extend(
            UNRELATED_REFS
                .into_iter()
                .enumerate()
                .map(|(index, ref_name)| {
                    (
                        ref_name,
                        first_index + index as u64 + 2,
                        seed.wrapping_add(index as u8 + 2),
                    )
                }),
        );
        records
    }

    fn log_only_table(algorithm: GitHashAlgorithm) -> Vec<u8> {
        log_table_for_index(algorithm, 1)
    }

    fn multi_level_log_table(algorithm: GitHashAlgorithm) -> (Vec<u8>, usize) {
        let count = 1_024;
        let records = (0..count)
            .rev()
            .map(|index| {
                let mut key = b"refs/heads/multi".to_vec();
                key.push(0);
                key.extend_from_slice(&(u64::MAX - (index + 1) as u64).to_be_bytes());
                let object_len = match algorithm {
                    GitHashAlgorithm::Sha1 => 20,
                    GitHashAlgorithm::Sha256 => 32,
                };
                let mut value = vec![((index % 254) + 1) as u8; object_len * 2];
                write_varint(&mut value, 4);
                value.extend_from_slice(b"name");
                write_varint(&mut value, 5);
                value.extend_from_slice(b"email");
                write_varint(&mut value, 1_700_000_000 + index as u64);
                value.extend_from_slice(&0_i16.to_be_bytes());
                write_varint(&mut value, 8);
                value.extend_from_slice(b"message\n");
                ReftableEncodedRecord {
                    key,
                    value_type: 1,
                    value,
                    object_ids: Vec::new(),
                }
            })
            .collect::<Vec<_>>();
        (
            encode_reftable(
                algorithm,
                1,
                count as u64,
                &[],
                &records,
                ReftableWriteOptions {
                    block_size: 256,
                    restart_interval: 2,
                    index_objects: false,
                },
            )
            .expect("encode multi-level log reftable"),
            count,
        )
    }

    fn unindexed_variable_log_table() -> (Vec<u8>, usize) {
        for block_size in (256..=1024).step_by(16) {
            for count in 4..=64 {
                let records = (0..count)
                    .rev()
                    .map(|index| {
                        let mut key = b"refs/heads/unindexed".to_vec();
                        key.push(0);
                        key.extend_from_slice(&(u64::MAX - (index + 1) as u64).to_be_bytes());
                        let mut value = vec![((index % 254) + 1) as u8; 40];
                        write_varint(&mut value, 4);
                        value.extend_from_slice(b"name");
                        write_varint(&mut value, 5);
                        value.extend_from_slice(b"email");
                        write_varint(&mut value, 1_700_000_000 + index as u64);
                        value.extend_from_slice(&0_i16.to_be_bytes());
                        let message_len = 80 + index % 41;
                        write_varint(&mut value, message_len as u64);
                        value.extend(
                            (0..message_len)
                                .map(|offset| b'!' + ((index * 31 + offset * 17) % 94) as u8),
                        );
                        ReftableEncodedRecord {
                            key,
                            value_type: 1,
                            value,
                            object_ids: Vec::new(),
                        }
                    })
                    .collect::<Vec<_>>();
                let bytes = encode_reftable(
                    GitHashAlgorithm::Sha1,
                    1,
                    count as u64,
                    &[],
                    &records,
                    ReftableWriteOptions {
                        block_size,
                        restart_interval: 2,
                        index_objects: false,
                    },
                )
                .expect("encode unindexed log reftable");
                let footer_start = bytes.len() - FOOTER_V1_LEN;
                let log_index_position = read_u64(&bytes[footer_start + HEADER_V1_LEN + 32..])
                    .expect("log index position");
                if bytes[HEADER_V1_LEN] == b'g' && log_index_position == 0 {
                    return (bytes, count);
                }
            }
        }
        panic!("failed to create a multi-block unindexed log table");
    }

    fn write_stack(dir: &Path, name: &str, bytes: &[u8]) {
        fs::write(dir.join(name), bytes).expect("write reftable table");
        fs::write(dir.join("tables.list"), format!("{name}\n")).expect("write tables.list");
    }

    fn write_stack_tables(dir: &Path, tables: &[(&str, Vec<u8>)]) {
        let mut names = String::new();
        for (name, bytes) in tables {
            fs::write(dir.join(name), bytes).expect("write reftable table");
            names.push_str(name);
            names.push('\n');
        }
        fs::write(dir.join("tables.list"), names).expect("write tables.list");
    }

    fn assert_rejected(name: &str, bytes: &[u8], algorithm: GitHashAlgorithm) {
        let dir = tempdir().expect("tempdir");
        write_stack(dir.path(), name, bytes);
        let error = open_stack(dir.path(), algorithm).expect_err("malformed table accepted");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{error}");
    }

    fn assert_list_rejected(name: &str, algorithm: GitHashAlgorithm) {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("tables.list"), format!("{name}\n"))
            .expect("write invalid tables.list");
        let error = open_stack(dir.path(), algorithm).expect_err("malformed tables.list accepted");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{error}");
    }

    fn update_footer_crc(bytes: &mut [u8], footer_len: usize) {
        let footer_start = bytes.len() - footer_len;
        let mut hasher = Crc32Hasher::new();
        hasher.update(&bytes[footer_start..bytes.len() - 4]);
        let checksum_start = bytes.len() - 4;
        bytes[checksum_start..].copy_from_slice(&hasher.finalize().to_be_bytes());
    }

    fn reheader_table_range(
        mut bytes: Vec<u8>,
        algorithm: GitHashAlgorithm,
        min_index: u64,
        max_index: u64,
    ) -> Vec<u8> {
        let footer_len = match algorithm {
            GitHashAlgorithm::Sha1 => FOOTER_V1_LEN,
            GitHashAlgorithm::Sha256 => FOOTER_V2_LEN,
        };
        bytes[8..16].copy_from_slice(&min_index.to_be_bytes());
        bytes[16..24].copy_from_slice(&max_index.to_be_bytes());
        let footer_start = bytes.len() - footer_len;
        bytes[footer_start + 8..footer_start + 16].copy_from_slice(&min_index.to_be_bytes());
        bytes[footer_start + 16..footer_start + 24].copy_from_slice(&max_index.to_be_bytes());
        update_footer_crc(&mut bytes, footer_len);
        bytes
    }

    fn first_index_block(bytes: &[u8], section_end: u64) -> u64 {
        let mut position = 0_u64;
        let mut first = true;
        loop {
            let header_offset = if first { HEADER_V1_LEN } else { 0 };
            let header_start = usize::try_from(position).expect("index block position");
            let block_start = header_start + header_offset;
            assert!(block_start + 4 <= bytes.len());
            let block_type = bytes[block_start];
            let block_len = (usize::from(bytes[block_start + 1]) << 16)
                | (usize::from(bytes[block_start + 2]) << 8)
                | usize::from(bytes[block_start + 3]);
            if block_type == b'i' {
                assert!(position < section_end);
                return position;
            }
            let block_end = if first {
                block_len as u64
            } else {
                position + block_len as u64
            };
            let next = if first {
                ((block_end + 127) / 128) * 128
            } else {
                position + 128
            };
            assert!(next > block_end && next <= section_end);
            position = next;
            first = false;
        }
    }

    fn promote_empty_sha1_to_v2(mut bytes: Vec<u8>) -> Vec<u8> {
        assert_eq!(bytes.len(), HEADER_V1_LEN + FOOTER_V1_LEN);
        let old_footer_start = bytes.len() - FOOTER_V1_LEN;
        let old_footer = bytes.split_off(old_footer_start);
        bytes[4] = 2;
        bytes.extend_from_slice(b"sha1");
        let mut footer = old_footer;
        footer[4] = 2;
        footer.splice(HEADER_V1_LEN..HEADER_V1_LEN, b"sha1".iter().copied());
        bytes.extend_from_slice(&footer);
        update_footer_crc(&mut bytes, FOOTER_V2_LEN);
        bytes
    }

    #[test]
    fn positional_reads_do_not_move_shared_file_cursor() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("positional-read");
        fs::write(&path, b"0123456789").expect("write positional-read fixture");
        let mut file = File::open(&path).expect("open positional-read fixture");
        file.seek(SeekFrom::Start(7)).expect("position file");
        let before = file.stream_position().expect("read cursor");
        let mut bytes = [0_u8; 3];
        read_exact_at(&file, &mut bytes, 2).expect("positional read");
        assert_eq!(&bytes, b"234");
        assert_eq!(file.stream_position().expect("read cursor"), before);
    }

    #[test]
    fn accepts_v1_sha1_v2_sha1_and_v2_sha256_envelopes() {
        let cases = [
            (
                SHA1_TABLE,
                GitHashAlgorithm::Sha1,
                empty_table(GitHashAlgorithm::Sha1, 1, 1),
            ),
            (
                SHA1_TABLE,
                GitHashAlgorithm::Sha1,
                promote_empty_sha1_to_v2(empty_table(GitHashAlgorithm::Sha1, 1, 1)),
            ),
            (
                SHA256_TABLE,
                GitHashAlgorithm::Sha256,
                empty_table(GitHashAlgorithm::Sha256, 1, 1),
            ),
        ];
        for (name, algorithm, bytes) in cases {
            let dir = tempdir().expect("tempdir");
            write_stack(dir.path(), name, &bytes);
            let snapshot = open_stack(dir.path(), algorithm).expect("valid envelope rejected");
            assert_eq!(snapshot.tables().len(), 1);
            assert_eq!(snapshot.tables()[0].name, name);
            assert!(
                snapshot.tables()[0]
                    .read_refs()
                    .expect("valid empty table refs rejected")
                    .is_empty()
            );
        }
    }

    #[test]
    fn reads_all_fixed_ref_blocks_when_section_ends_after_padding() {
        let dir = tempdir().expect("tempdir");
        let bytes = table_with_refs(GitHashAlgorithm::Sha1, 1, 1, 64);
        write_stack(dir.path(), SHA1_TABLE, &bytes);
        let snapshot = open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("open table");
        let refs = snapshot.tables()[0].read_refs().expect("read ref blocks");
        assert_eq!(refs.len(), 64);
    }

    #[test]
    fn scans_first_log_block_from_zero_log_position_sentinel() {
        let dir = tempdir().expect("tempdir");
        let bytes = log_only_table(GitHashAlgorithm::Sha1);
        write_stack(dir.path(), SHA1_TABLE, &bytes);
        let snapshot = open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("open log table");
        let table = &snapshot.tables()[0];
        assert_eq!(table.footer.log_position, table.header.len as u64);
        let logs = table.read_logs().expect("read first log block");
        assert_eq!(logs.len(), 1);
        match &logs[0] {
            ReftableParsedLogRecord::Update(record) => {
                assert_eq!(record.ref_name, "refs/heads/log-only");
                assert_eq!(record.message, "message\n");
            }
            ReftableParsedLogRecord::Deletion { .. } => panic!("expected log update"),
        }
    }

    #[test]
    fn reads_stock_format_multi_level_log_index() {
        let dir = tempdir().expect("tempdir");
        let (bytes, count) = multi_level_log_table(GitHashAlgorithm::Sha1);
        write_stack(dir.path(), MULTI_LEVEL_LOG_TABLE, &bytes);
        let snapshot =
            open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("multi-level log index rejected");
        let table = &snapshot.tables()[0];
        let scan = table
            .scan_logs(
                table.footer.log_position,
                table.next_boundary(table.footer.log_position),
                |_, _, _| Ok(()),
            )
            .expect("scan multi-level log data");
        let first_index = scan
            .first_index_position
            .expect("multi-level log index not detected");
        assert!(first_index < table.footer.log_index_position);
        assert_eq!(
            table.read_logs().expect("read multi-level logs").len(),
            count
        );
    }

    #[test]
    fn reftable_log_cursor_yields_oldest_to_newest_across_blocks() {
        let dir = tempdir().expect("tempdir");
        let (bytes, count) = multi_level_log_table(GitHashAlgorithm::Sha1);
        write_stack(dir.path(), MULTI_LEVEL_LOG_TABLE, &bytes);
        let snapshot =
            open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("multi-level log index rejected");
        let mut cursor = ReftableLogCursor::new_oldest_to_newest(&snapshot, "refs/heads/multi")
            .expect("create reftable log cursor");
        for expected in 1..=count as u64 {
            let record = cursor
                .next()
                .expect("read reftable log cursor")
                .expect("missing reftable log record");
            assert_eq!(record.update_index, expected);
        }
        assert!(cursor.next().expect("finish reftable log cursor").is_none());
    }

    #[test]
    fn unindexed_log_cursor_scans_variable_blocks_newest_to_oldest() {
        let dir = tempdir().expect("tempdir");
        let (bytes, count) = unindexed_variable_log_table();
        let table_name = format!("0x000000000001-0x{count:012x}-unindexed.log");
        write_stack(dir.path(), &table_name, &bytes);
        let snapshot = open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("open table");
        let table = &snapshot.tables()[0];
        assert_eq!(table.footer.log_index_position, 0);
        let mut cursor = ReftableLogCursor::new_newest_to_oldest(&snapshot, "refs/heads/unindexed")
            .expect("create unindexed newest-first cursor");
        for expected in (1..=count as u64).rev() {
            let record = cursor
                .next()
                .expect("read unindexed cursor")
                .expect("missing unindexed log record");
            assert_eq!(record.update_index, expected);
        }
        assert!(cursor.next().expect("finish unindexed cursor").is_none());
        assert!(
            ReftableLogCursor::new_oldest_to_newest(&snapshot, "refs/heads/unindexed").is_err()
        );
    }

    #[test]
    fn reftable_log_cursor_merges_tables_in_update_order_without_record_storage() {
        let dir = tempdir().expect("tempdir");
        let first_name = "0x000000000001-0x000000000001-first.log";
        let second_name = "0x000000000002-0x000000000002-second.log";
        fs::write(
            dir.path().join(first_name),
            log_table_for_index(GitHashAlgorithm::Sha1, 1),
        )
        .expect("write first log table");
        fs::write(
            dir.path().join(second_name),
            log_table_for_index(GitHashAlgorithm::Sha1, 2),
        )
        .expect("write second log table");
        fs::write(
            dir.path().join("tables.list"),
            format!("{first_name}\n{second_name}\n"),
        )
        .expect("write multi-table list");
        let snapshot = open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("open stack");
        let mut cursor = ReftableLogCursor::new_newest_to_oldest(&snapshot, "refs/heads/log-only")
            .expect("create newest-first cursor");
        assert_eq!(
            cursor
                .next()
                .expect("read newest record")
                .expect("missing newest record")
                .update_index,
            2
        );
        assert_eq!(
            cursor
                .next()
                .expect("read oldest record")
                .expect("missing oldest record")
                .update_index,
            1
        );
        assert!(cursor.next().expect("finish cursor").is_none());
    }

    #[test]
    fn reftable_log_cursor_heap_merges_interleaved_tombstones_in_both_directions() {
        for algorithm in [GitHashAlgorithm::Sha1, GitHashAlgorithm::Sha256] {
            let dir = tempdir().expect("tempdir");
            let first_name = "0x000000000001-0x000000000001-first.log";
            let second_name = "0x000000000002-0x000000000002-second.log";
            let third_name = "0x000000000003-0x000000000003-third.log";
            let fourth_name = "0x000000000004-0x000000000004-fourth.log";
            write_stack_tables(
                dir.path(),
                &[
                    (
                        first_name,
                        log_table_for_records(
                            algorithm,
                            1,
                            1,
                            &indexed_interleaved_records("refs/heads/log-only", 1, 0x11),
                        ),
                    ),
                    (
                        second_name,
                        log_table_for_records(
                            algorithm,
                            2,
                            2,
                            &indexed_interleaved_records("refs/heads/log-only", 2, 0x22),
                        ),
                    ),
                    (
                        third_name,
                        log_table_for_records(
                            algorithm,
                            3,
                            3,
                            &indexed_interleaved_records("refs/heads/log-only", 3, 0x33),
                        ),
                    ),
                    (
                        fourth_name,
                        deletion_table_for_records(
                            algorithm,
                            4,
                            4,
                            &[
                                ("refs/heads/log-only", 2),
                                ("refs/heads/unrelated-a", 5),
                                ("refs/heads/unrelated-b", 6),
                                ("refs/heads/unrelated-c", 7),
                                ("refs/heads/unrelated-d", 8),
                                ("refs/heads/unrelated-e", 9),
                                ("refs/heads/unrelated-f", 10),
                                ("refs/heads/unrelated-g", 11),
                                ("refs/heads/unrelated-h", 12),
                                ("refs/heads/unrelated-i", 13),
                                ("refs/heads/unrelated-j", 14),
                                ("refs/heads/unrelated-k", 15),
                                ("refs/heads/unrelated-l", 16),
                                ("refs/heads/unrelated-m", 17),
                                ("refs/heads/unrelated-n", 18),
                                ("refs/heads/unrelated-o", 19),
                                ("refs/heads/unrelated-p", 20),
                                ("refs/heads/unrelated-q", 21),
                                ("refs/heads/unrelated-r", 22),
                                ("refs/heads/unrelated-s", 23),
                                ("refs/heads/unrelated-t", 24),
                                ("refs/heads/unrelated-u", 25),
                                ("refs/heads/unrelated-v", 26),
                                ("refs/heads/unrelated-w", 27),
                                ("refs/heads/unrelated-x", 28),
                            ],
                        ),
                    ),
                ],
            );
            let snapshot = open_stack(dir.path(), algorithm).expect("open stacked logs");
            let mut newest =
                ReftableLogCursor::new_newest_to_oldest(&snapshot, "refs/heads/log-only")
                    .expect("create newest-first heap cursor");
            let mut newest_indexes = Vec::new();
            while let Some(record) = newest.next().expect("read newest-first heap cursor") {
                newest_indexes.push(record.update_index);
            }
            assert_eq!(newest_indexes, [4, 3, 1]);
        }
    }

    #[test]
    fn reftable_log_cursor_indexed_order_is_bidirectional_for_sha1_and_sha256() {
        for algorithm in [GitHashAlgorithm::Sha1, GitHashAlgorithm::Sha256] {
            let dir = tempdir().expect("tempdir");
            let (bytes, count) = multi_level_log_table(algorithm);
            write_stack(dir.path(), MULTI_LEVEL_LOG_TABLE, &bytes);
            let snapshot = open_stack(dir.path(), algorithm).expect("open indexed log table");
            for order in [
                ReftableLogOrder::OldestToNewest,
                ReftableLogOrder::NewestToOldest,
            ] {
                let mut cursor =
                    ReftableLogCursor::new_with_order(&snapshot, "refs/heads/multi", order, false)
                        .expect("create indexed bidirectional cursor");
                let mut indexes = Vec::new();
                while let Some(record) = cursor.next().expect("read indexed cursor") {
                    indexes.push(record.update_index);
                }
                let expected = match order {
                    ReftableLogOrder::OldestToNewest => (1..=count as u64).collect::<Vec<_>>(),
                    ReftableLogOrder::NewestToOldest => {
                        (1..=count as u64).rev().collect::<Vec<_>>()
                    }
                };
                assert_eq!(indexes, expected);
            }
        }
    }

    #[test]
    fn reftable_log_cursor_prefers_newer_summary_over_older_tombstone() {
        for algorithm in [GitHashAlgorithm::Sha1, GitHashAlgorithm::Sha256] {
            let dir = tempdir().expect("tempdir");
            let older_name = "0x000000000007-0x000000000007-oldertombstone.log";
            let newer_name = "0x000000000008-0x000000000008-newersummary.log";
            write_stack_tables(
                dir.path(),
                &[
                    (
                        older_name,
                        deletion_table_for_records(algorithm, 7, 7, &[("refs/heads/log-only", 7)]),
                    ),
                    (
                        newer_name,
                        log_table_for_records(algorithm, 8, 8, &[("refs/heads/log-only", 7, 0x22)]),
                    ),
                ],
            );
            let snapshot = open_stack(dir.path(), algorithm).expect("open stacked logs");
            let mut cursor =
                ReftableLogCursor::new_newest_to_oldest(&snapshot, "refs/heads/log-only")
                    .expect("create summary-preferred cursor");
            let record = cursor
                .next()
                .expect("read summary-preferred record")
                .expect("missing newer summary");
            assert_eq!(record.update_index, 7);
            assert!(
                cursor
                    .next()
                    .expect("finish summary-preferred cursor")
                    .is_none()
            );
        }
    }

    fn corrupt_log_index_header(path: &Path, position: u64) {
        let mut file = OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open reftable for corruption");
        file.seek(SeekFrom::Start(position))
            .expect("seek to log index");
        file.write_all(b"x").expect("corrupt log index header");
        file.flush().expect("flush corrupted log index");
    }

    fn assert_terminal_error_repeats(
        cursor: &mut ReftableLogCursor<'_>,
    ) -> (io::ErrorKind, String) {
        let first = cursor.next().expect_err("corrupt cursor emitted a record");
        let second = cursor
            .next()
            .expect_err("poisoned cursor stopped returning its terminal error");
        assert_eq!(second.kind(), first.kind());
        assert_eq!(second.to_string(), first.to_string());
        (first.kind(), first.to_string())
    }

    #[test]
    fn reftable_log_cursor_poison_is_terminal_after_later_table_init_error() {
        let dir = tempdir().expect("tempdir");
        let first_name = "0x000000000001-0x000000000400-first.log";
        let second_name = "0x000000000401-0x000000000800-second.log";
        let (first_bytes, _) = multi_level_log_table(GitHashAlgorithm::Sha1);
        write_stack_tables(
            dir.path(),
            &[
                (first_name, first_bytes.clone()),
                (
                    second_name,
                    reheader_table_range(first_bytes, GitHashAlgorithm::Sha1, 1025, 2048),
                ),
            ],
        );
        let snapshot = open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("open stacked logs");
        let second_index = snapshot.tables()[1].footer.log_index_position;
        let mut cursor = ReftableLogCursor::new_oldest_to_newest(&snapshot, "refs/heads/multi")
            .expect("create lazy cursor");
        corrupt_log_index_header(&dir.path().join(second_name), second_index);
        let (kind, message) = assert_terminal_error_repeats(&mut cursor);
        assert_eq!(kind, io::ErrorKind::InvalidData);
        assert!(message.contains("reftable log index target is not an index block"));
    }

    #[test]
    fn reftable_log_cursor_poison_survives_partial_group_refill() {
        let dir = tempdir().expect("tempdir");
        let first_name = "0x000000000001-0x000000000400-first.log";
        let second_name = "0x000000000401-0x000000000800-second.log";
        let (first_bytes, _) = multi_level_log_table(GitHashAlgorithm::Sha1);
        write_stack_tables(
            dir.path(),
            &[
                (first_name, first_bytes.clone()),
                (
                    second_name,
                    reheader_table_range(first_bytes, GitHashAlgorithm::Sha1, 1025, 2048),
                ),
            ],
        );
        let snapshot = open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("open stacked logs");
        let mut cursor = ReftableLogCursor::new_oldest_to_newest(&snapshot, "refs/heads/multi")
            .expect("create grouped cursor");
        cursor.initialize_heap().expect("prime grouped heads");
        cursor.tables[1].pending.clear();
        let second_index = snapshot.tables()[1].footer.log_index_position;
        corrupt_log_index_header(&dir.path().join(second_name), second_index);
        let (kind, message) = assert_terminal_error_repeats(&mut cursor);
        assert_eq!(kind, io::ErrorKind::InvalidData);
        assert!(message.contains("reftable table changed after validation"));
    }

    #[test]
    fn reftable_log_cursor_poison_is_lazy_for_index_corruption() {
        let dir = tempdir().expect("tempdir");
        write_stack(
            dir.path(),
            MULTI_LEVEL_LOG_TABLE,
            &multi_level_log_table(GitHashAlgorithm::Sha1).0,
        );
        let snapshot = open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("open log table");
        let cursor = ReftableLogCursor::new_newest_to_oldest(&snapshot, "refs/heads/multi")
            .expect("constructor must remain lazy");
        let mut cursor = cursor;
        let index = snapshot.tables()[0].footer.log_index_position;
        corrupt_log_index_header(&dir.path().join(MULTI_LEVEL_LOG_TABLE), index);
        let (kind, message) = assert_terminal_error_repeats(&mut cursor);
        assert_eq!(kind, io::ErrorKind::InvalidData);
        assert!(message.contains("reftable log index target is not an index block"));
    }

    #[test]
    fn newest_log_index_decodes_each_index_entry_once() {
        let dir = tempdir().expect("tempdir");
        let (bytes, count) = multi_level_log_table(GitHashAlgorithm::Sha1);
        write_stack(dir.path(), MULTI_LEVEL_LOG_TABLE, &bytes);
        let snapshot =
            open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("multi-level log index rejected");
        let mut cursor = ReftableLogCursor::new_newest_to_oldest(&snapshot, "refs/heads/multi")
            .expect("create newest-first indexed cursor");
        let mut records = 0;
        while cursor
            .next()
            .expect("read newest-first indexed cursor")
            .is_some()
        {
            records += 1;
        }
        assert_eq!(records, count);
        let stats = cursor.index_traversal_stats();
        assert!(stats.block_initializations > 0);
        assert!(stats.entry_decodes > 0);
        assert_eq!(stats.block_initializations, stats.initialized_blocks.len());
        assert_eq!(stats.entry_decodes, stats.decoded_entries.len());
        assert!(stats.entry_decodes <= count);
        assert!(stats.block_initializations <= stats.entry_decodes);
    }

    #[test]
    fn reftable_log_cursor_suppresses_same_key_older_table_tombstone() {
        for algorithm in [GitHashAlgorithm::Sha1, GitHashAlgorithm::Sha256] {
            let dir = tempdir().expect("tempdir");
            let older_name = "0x000000000007-0x000000000007-older.log";
            let newer_name = "0x000000000008-0x000000000008-newer.log";
            fs::write(
                dir.path().join(older_name),
                log_table_for_index(algorithm, 7),
            )
            .expect("write older log table");
            fs::write(
                dir.path().join(newer_name),
                deletion_table_for_indices(algorithm, 8, 7),
            )
            .expect("write newer tombstone table");
            fs::write(
                dir.path().join("tables.list"),
                format!("{older_name}\n{newer_name}\n"),
            )
            .expect("write stacked tables list");

            let snapshot = open_stack(dir.path(), algorithm).expect("open stack");
            let mut cursor =
                ReftableLogCursor::new_newest_to_oldest(&snapshot, "refs/heads/log-only")
                    .expect("create merged log cursor");
            assert!(cursor.next().expect("read tombstoned record").is_none());
        }
    }

    #[test]
    fn rejects_long_invalid_utf8_reftable_log_message_after_bounded_decode() {
        let dir = tempdir().expect("tempdir");
        let mut key = b"refs/heads/long-message".to_vec();
        key.push(0);
        key.extend_from_slice(&(u64::MAX - 1).to_be_bytes());
        let mut value = vec![1; 20];
        value.extend(std::iter::repeat_n(2, 20));
        write_varint(&mut value, 4);
        value.extend_from_slice(b"name");
        write_varint(&mut value, 5);
        value.extend_from_slice(b"email");
        write_varint(&mut value, 1_700_000_000);
        value.extend_from_slice(&0_i16.to_be_bytes());
        let message_len = 2 * 1024 * 1024 + 1;
        write_varint(&mut value, message_len as u64);
        value.extend(std::iter::repeat_n(b'x', message_len - 1));
        value.push(0xff);
        let bytes = encode_reftable(
            GitHashAlgorithm::Sha1,
            1,
            1,
            &[],
            &[ReftableEncodedRecord {
                key,
                value_type: 1,
                value,
                object_ids: Vec::new(),
            }],
            ReftableWriteOptions {
                block_size: 3 * 1024 * 1024,
                restart_interval: 2,
                index_objects: false,
            },
        )
        .expect("encode long log table");
        write_stack(dir.path(), SHA1_TABLE, &bytes);
        let snapshot = open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("open table");
        let mut cursor =
            ReftableLogCursor::new_newest_to_oldest(&snapshot, "refs/heads/long-message")
                .expect("create long-message cursor");
        let error = cursor.next().expect_err("invalid UTF-8 message accepted");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("non-utf8 reftable log message"));
    }

    #[test]
    fn validates_non_empty_sha256_object_section_with_32_byte_ids() {
        let dir = tempdir().expect("tempdir");
        let mut bytes = table_with_sha256_objects();
        let footer_start = bytes.len() - FOOTER_V2_LEN;
        let object_field = footer_start + HEADER_V2_LEN + 8;
        let encoded = u64::from_be_bytes(
            bytes[object_field..object_field + 8]
                .try_into()
                .expect("object footer field"),
        );
        assert_eq!(encoded & 0x1f, 0);
        bytes[object_field..object_field + 8].copy_from_slice(&(encoded - 32).to_be_bytes());
        update_footer_crc(&mut bytes, FOOTER_V2_LEN);
        write_stack(dir.path(), SHA256_TABLE, &bytes);
        let snapshot = open_stack(dir.path(), GitHashAlgorithm::Sha256).expect("open table");
        let table = &snapshot.tables()[0];
        assert_eq!(table.footer.obj_id_len, 32);
        assert_ne!(table.footer.obj_position, 0);
        assert_eq!(table.read_refs().expect("read sha256 refs").len(), 64);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn snapshot_uses_validated_handle_after_path_replacement() {
        let dir = tempdir().expect("tempdir");
        let bytes = table_with_refs(GitHashAlgorithm::Sha1, 1, 1, 1);
        write_stack(dir.path(), SHA1_TABLE, &bytes);
        let snapshot = open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("open table");
        let replacement = dir.path().join("replacement.ref");
        fs::write(&replacement, vec![0_u8; bytes.len()]).expect("write replacement");
        fs::remove_file(dir.path().join(SHA1_TABLE)).expect("remove original path");
        fs::rename(replacement, dir.path().join(SHA1_TABLE)).expect("replace table path");
        assert_eq!(
            snapshot.tables()[0]
                .read_refs()
                .expect("read validated open handle")
                .len(),
            1
        );
    }

    #[test]
    fn rejects_unsafe_table_names_and_out_of_order_lists() {
        for name in [
            "../escape.ref",
            "0x000000000001-0x000000000001-.ref",
            "0x000000000001-0x000000000001-test.refabc",
            "0x000000000001-0x000000000001-test.ref/child",
        ] {
            assert_list_rejected(name, GitHashAlgorithm::Sha1);
        }

        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("tables.list"),
            "0x000000000002-0x000000000002-test.ref\n0x000000000001-0x000000000001-test.ref\n",
        )
        .expect("write out-of-order tables.list");
        let error = open_stack(dir.path(), GitHashAlgorithm::Sha1)
            .expect_err("out-of-order tables.list accepted");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_truncated_headers_and_footers() {
        let bytes = empty_table(GitHashAlgorithm::Sha1, 1, 1);
        let mut truncated_header = bytes.clone();
        truncated_header.truncate(HEADER_V1_LEN - 1);
        assert_rejected(SHA1_TABLE, &truncated_header, GitHashAlgorithm::Sha1);

        let mut truncated_footer = bytes;
        truncated_footer.pop();
        assert_rejected(SHA1_TABLE, &truncated_footer, GitHashAlgorithm::Sha1);
    }

    #[test]
    fn rejects_header_footer_mismatch_and_crc_mismatch() {
        let bytes = empty_table(GitHashAlgorithm::Sha1, 1, 1);
        let footer_start = bytes.len() - FOOTER_V1_LEN;

        let mut mismatched = bytes.clone();
        mismatched[footer_start + 16] ^= 1;
        update_footer_crc(&mut mismatched, FOOTER_V1_LEN);
        assert_rejected(SHA1_TABLE, &mismatched, GitHashAlgorithm::Sha1);

        let mut bad_crc = bytes;
        let last = bad_crc.len() - 1;
        bad_crc[last] ^= 1;
        assert_rejected(SHA1_TABLE, &bad_crc, GitHashAlgorithm::Sha1);
    }

    #[test]
    fn rejects_invalid_offsets_lengths_and_padding() {
        let bytes = table_with_refs(GitHashAlgorithm::Sha1, 1, 1, 1);
        let footer_start = bytes.len() - FOOTER_V1_LEN;

        let mut invalid_offset = bytes.clone();
        invalid_offset[footer_start + HEADER_V1_LEN..footer_start + HEADER_V1_LEN + 8]
            .copy_from_slice(&1_u64.to_be_bytes());
        update_footer_crc(&mut invalid_offset, FOOTER_V1_LEN);
        assert_rejected(SHA1_TABLE, &invalid_offset, GitHashAlgorithm::Sha1);

        let mut invalid_length = bytes;
        invalid_length[25..28].copy_from_slice(&[0xff, 0xff, 0xff]);
        assert_rejected(SHA1_TABLE, &invalid_length, GitHashAlgorithm::Sha1);

        let mut padded = table_with_refs(GitHashAlgorithm::Sha1, 1, 1, 32);
        let footer_start = padded.len() - FOOTER_V1_LEN;
        let ref_index_position = u64::from_be_bytes(
            padded[footer_start + HEADER_V1_LEN..footer_start + HEADER_V1_LEN + 8]
                .try_into()
                .expect("ref index position"),
        );
        assert!(
            ref_index_position > 0,
            "fixture must contain multiple ref blocks"
        );
        let first_block_len = (usize::from(padded[25]) << 16)
            | (usize::from(padded[26]) << 8)
            | usize::from(padded[27]);
        assert!(first_block_len < 128, "fixture must contain block padding");
        padded[first_block_len] = 1;
        assert_rejected(SHA1_TABLE, &padded, GitHashAlgorithm::Sha1);
    }

    #[test]
    fn rejects_index_root_mismatch_after_recomputing_footer_crc() {
        let mut bytes = table_with_refs(GitHashAlgorithm::Sha1, 1, 1, 256);
        let footer_start = bytes.len() - FOOTER_V1_LEN;
        let ref_index_field = footer_start + HEADER_V1_LEN;
        let ref_index_position = u64::from_be_bytes(
            bytes[ref_index_field..ref_index_field + 8]
                .try_into()
                .expect("ref index position"),
        );
        let mutated_position = ref_index_position
            .checked_add(128)
            .expect("mutated ref index position");
        assert!(mutated_position < footer_start as u64);
        bytes[ref_index_field..ref_index_field + 8]
            .copy_from_slice(&mutated_position.to_be_bytes());
        update_footer_crc(&mut bytes, FOOTER_V1_LEN);
        let dir = tempdir().expect("tempdir");
        write_stack(dir.path(), SHA1_TABLE, &bytes);
        let error = open_stack(dir.path(), GitHashAlgorithm::Sha1)
            .expect_err("mutated index root accepted");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("footer index root"),
            "unexpected rejection reason: {error}"
        );
    }

    #[test]
    fn rejects_footer_root_set_to_exact_first_lower_level_block() {
        let mut bytes = table_with_refs(GitHashAlgorithm::Sha1, 1, 1, 256);
        let footer_start = bytes.len() - FOOTER_V1_LEN;
        let ref_index_field = footer_start + HEADER_V1_LEN;
        let ref_index_position = u64::from_be_bytes(
            bytes[ref_index_field..ref_index_field + 8]
                .try_into()
                .expect("ref index position"),
        );
        let first_lower_level = first_index_block(&bytes, ref_index_position);
        assert!(first_lower_level < ref_index_position);
        bytes[ref_index_field..ref_index_field + 8]
            .copy_from_slice(&first_lower_level.to_be_bytes());
        update_footer_crc(&mut bytes, FOOTER_V1_LEN);
        let dir = tempdir().expect("tempdir");
        write_stack(dir.path(), SHA1_TABLE, &bytes);
        let error = open_stack(dir.path(), GitHashAlgorithm::Sha1)
            .expect_err("lower-level footer index root accepted");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("footer index root"),
            "unexpected rejection reason: {error}"
        );
    }

    #[test]
    fn rejects_multi_level_log_index_root_mutations_after_recomputing_crc() {
        let (bytes, _) = multi_level_log_table(GitHashAlgorithm::Sha1);
        let dir = tempdir().expect("tempdir");
        write_stack(dir.path(), MULTI_LEVEL_LOG_TABLE, &bytes);
        let snapshot =
            open_stack(dir.path(), GitHashAlgorithm::Sha1).expect("multi-level log index rejected");
        let table = &snapshot.tables()[0];
        let scan = table
            .scan_logs(
                table.footer.log_position,
                table.next_boundary(table.footer.log_position),
                |_, _, _| Ok(()),
            )
            .expect("scan multi-level log data");
        let first_index = scan
            .first_index_position
            .expect("multi-level log index not detected");
        let root = table.footer.log_index_position;
        assert!(first_index < root);
        drop(snapshot);

        let footer_start = bytes.len() - FOOTER_V1_LEN;
        let log_index_field = footer_start + HEADER_V1_LEN + 32;

        for mutated_root in [first_index, root + 1] {
            assert!(mutated_root < footer_start as u64);
            let mut mutated = bytes.clone();
            mutated[log_index_field..log_index_field + 8]
                .copy_from_slice(&mutated_root.to_be_bytes());
            update_footer_crc(&mut mutated, FOOTER_V1_LEN);
            fs::write(dir.path().join(MULTI_LEVEL_LOG_TABLE), mutated)
                .expect("write mutated table");
            let error = open_stack(dir.path(), GitHashAlgorithm::Sha1)
                .expect_err("mutated log index root accepted");
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(
                error.to_string().contains("log") || error.to_string().contains("index"),
                "unexpected rejection reason: {error}"
            );
        }
    }
}
