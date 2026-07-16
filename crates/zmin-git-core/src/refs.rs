use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use flate2::read::ZlibDecoder;

use crate::{
    GitHashAlgorithm, GitObjectKind, GitObjectStore, LooseObjectStore, ObjectId, decode_tag,
    reftable_writer::{
        ReftableEncodedRecord, ReftableWriteOptions, encode_reftable as encode_reftable_table,
    },
};

const PACKED_REFS_IO_BUFFER_CAPACITY: usize = 64 * 1024;
const PACKED_REF_LINE_INITIAL_CAPACITY: usize = 128;
const REFTABLE_HEADER_V1_LEN: usize = 24;
const REFTABLE_HEADER_V2_LEN: usize = 28;
const REFTABLE_BLOCK_HEADER_LEN: usize = 4;
const REFTABLE_RESTART_COUNT_LEN: usize = 2;
#[cfg(test)]
const REFTABLE_COMPACTION_TABLE_LIMIT: usize = 64;
const REFTABLE_DEFAULT_GEOMETRIC_FACTOR: u64 = 2;
const REFTABLE_FOOTER_V1_LEN: usize = 68;
const REFTABLE_FOOTER_V2_LEN: usize = 72;
const REFTABLE_TABLE_NAME_CAPACITY: usize = 45;
const REFTABLE_LOCK_TIMEOUT_ENV: &str = "ZMIN_REFTABLE_LOCK_TIMEOUT_MS";
const REFTABLE_BLOCK_SIZE_ENV: &str = "ZMIN_REFTABLE_BLOCK_SIZE";
const REFTABLE_INDEX_OBJECTS_ENV: &str = "ZMIN_REFTABLE_INDEX_OBJECTS";
const REFTABLE_RESTART_INTERVAL_ENV: &str = "ZMIN_REFTABLE_RESTART_INTERVAL";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefTarget {
    Direct(ObjectId),
    Symbolic(String),
}

#[derive(Debug, Clone)]
pub struct RefStore {
    git_dir: PathBuf,
    algorithm: GitHashAlgorithm,
    storage: RefStorageLocation,
}

#[derive(Debug, Clone)]
enum RefStorageLocation {
    Valid { kind: RefStorageKind, root: PathBuf },
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefStorageKind {
    Files,
    Reftable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRefsOptions {
    pub all: bool,
    pub prune: bool,
    pub auto: bool,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReftableLogRecord {
    pub ref_name: String,
    pub update_index: u64,
    pub old_id: ObjectId,
    pub new_id: ObjectId,
    pub name: String,
    pub email: String,
    pub timestamp: u64,
    pub timezone_offset: i16,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfoRef {
    pub id: ObjectId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RefPathState {
    Missing,
    Valid(RefTarget),
    BrokenFile,
    EmptyDir,
    BlockingDir,
}

impl RefStore {
    pub fn new(git_dir: impl Into<PathBuf>, algorithm: GitHashAlgorithm) -> Self {
        let git_dir = git_dir.into();
        let storage = resolve_ref_storage_location(&git_dir, true);
        Self {
            git_dir,
            algorithm,
            storage,
        }
    }

    pub fn new_without_environment(
        git_dir: impl Into<PathBuf>,
        algorithm: GitHashAlgorithm,
    ) -> Self {
        let git_dir = git_dir.into();
        let storage = resolve_ref_storage_location(&git_dir, false);
        Self {
            git_dir,
            algorithm,
            storage,
        }
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub fn new_with_storage_root(
        git_dir: impl Into<PathBuf>,
        storage_root: impl Into<PathBuf>,
        algorithm: GitHashAlgorithm,
        kind: RefStorageKind,
    ) -> Self {
        Self {
            git_dir: git_dir.into(),
            algorithm,
            storage: RefStorageLocation::Valid {
                kind,
                root: storage_root.into(),
            },
        }
    }

    pub fn storage_root_path(&self) -> io::Result<&Path> {
        self.storage_root()
    }

    pub fn uses_external_storage(&self) -> io::Result<bool> {
        Ok(self.storage_root()? != self.git_dir)
    }

    pub fn storage_kind(&self) -> io::Result<RefStorageKind> {
        match &self.storage {
            RefStorageLocation::Valid { kind, .. } => Ok(*kind),
            RefStorageLocation::Invalid(value) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid value for extensions.refStorage: {value}"),
            )),
        }
    }

    fn storage_root(&self) -> io::Result<&Path> {
        match &self.storage {
            RefStorageLocation::Valid { root, .. } => Ok(root),
            RefStorageLocation::Invalid(value) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid value for extensions.refStorage: {value}"),
            )),
        }
    }

    pub fn write_ref(&self, name: &str, id: &ObjectId) -> io::Result<()> {
        validate_storable_ref_name(name)?;
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match ref store",
            ));
        }
        if self.storage_kind()? == RefStorageKind::Reftable {
            self.write_reftable_ref_update(name, Some(RefTarget::Direct(id.clone())), true)?;
            return Ok(());
        }
        if name.starts_with("refs/") {
            self.ensure_no_ref_name_conflict(name)?;
        }
        let ref_path = self.ref_path(name);
        match inspect_ref_path(&ref_path, self.algorithm, name != "HEAD")? {
            RefPathState::Missing | RefPathState::EmptyDir if name.starts_with("refs/") => {
                if matches!(self.read_packed_ref(name)?, Some(RefTarget::Direct(current)) if current == *id)
                {
                    return Ok(());
                }
            }
            _ => {}
        }
        prepare_ref_path_for_write(&ref_path, self.algorithm, name != "HEAD")?;
        atomic_write(ref_path, format!("{}\n", id.to_hex()).as_bytes())
    }

    pub fn write_symbolic_ref(&self, name: &str, target: &str) -> io::Result<()> {
        if self.storage_kind()? == RefStorageKind::Reftable {
            validate_storable_ref_name(name)?;
            validate_storable_ref_name(target)?;
            self.write_reftable_ref_update(
                name,
                Some(RefTarget::Symbolic(target.to_owned())),
                true,
            )?;
            return Ok(());
        }
        if name == "HEAD" {
            return self.write_head_symbolic(target);
        }
        validate_storable_ref_name(name)?;
        validate_storable_ref_name(target)?;
        if name.starts_with("refs/") {
            self.ensure_no_ref_name_conflict(name)?;
        }
        prepare_ref_path_for_write(&self.ref_path(name), self.algorithm, name != "HEAD")?;
        atomic_write(self.ref_path(name), format!("ref: {target}\n").as_bytes())
    }

    pub fn delete_ref(&self, name: &str) -> io::Result<()> {
        validate_ref_lookup_name(name)?;
        if self.storage_kind()? == RefStorageKind::Reftable {
            if self.write_reftable_ref_update(name, None, false)? {
                return Ok(());
            }
            return Err(io::Error::new(io::ErrorKind::NotFound, "ref not found"));
        }
        let ref_path = self.ref_path(name);
        let state = inspect_ref_path(&ref_path, self.algorithm, name != "HEAD")?;
        let mut deleted = if name.starts_with("refs/") {
            delete_packed_ref(self.algorithm, self.storage_root()?, name)?
        } else {
            false
        };
        match state {
            RefPathState::Missing => {}
            RefPathState::Valid(_) | RefPathState::BrokenFile => {
                fs::remove_file(&ref_path)?;
                deleted = true;
                if name.starts_with("refs/") {
                    prune_empty_ref_parent_dirs(self.storage_root()?, &ref_path)?;
                }
            }
            RefPathState::EmptyDir => {
                fs::remove_dir_all(&ref_path)?;
                deleted = true;
                if name.starts_with("refs/") {
                    prune_empty_ref_parent_dirs(self.storage_root()?, &ref_path)?;
                }
            }
            RefPathState::BlockingDir => {
                return Err(io::Error::new(
                    io::ErrorKind::IsADirectory,
                    "non-empty ref directory",
                ));
            }
        }
        if deleted {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "ref not found"))
        }
    }

    pub fn read_ref(&self, name: &str) -> io::Result<RefTarget> {
        validate_ref_lookup_name(name)?;
        if self.storage_kind()? == RefStorageKind::Reftable {
            return self
                .read_reftable_ref(name)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "ref not found"));
        }
        match inspect_ref_path(&self.ref_path(name), self.algorithm, name != "HEAD")? {
            RefPathState::Valid(target) => Ok(target),
            RefPathState::Missing | RefPathState::EmptyDir if name.starts_with("refs/") => self
                .read_packed_ref(name)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "ref not found")),
            RefPathState::Missing | RefPathState::EmptyDir => {
                Err(io::Error::new(io::ErrorKind::NotFound, "ref not found"))
            }
            RefPathState::BrokenFile => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "reference broken",
            )),
            RefPathState::BlockingDir => Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                "non-empty ref directory",
            )),
        }
    }

    pub fn list_refs(&self, prefix: &str) -> io::Result<Vec<String>> {
        let mut refs = Vec::new();
        self.for_each_ref_name(prefix, |name| {
            refs.push(name.to_owned());
            Ok::<(), io::Error>(())
        })?;
        Ok(refs)
    }

    pub fn list_ref_targets(&self, prefix: &str) -> io::Result<Vec<(String, RefTarget)>> {
        validate_ref_prefix(prefix)?;
        if self.storage_kind()? == RefStorageKind::Reftable {
            return Ok(self
                .read_reftable_refs()?
                .into_iter()
                .filter(|(name, _)| name.starts_with(prefix))
                .collect());
        }
        self.list_refs(prefix)?
            .into_iter()
            .map(|name| self.read_ref(&name).map(|target| (name, target)))
            .collect()
    }

    pub fn list_root_refs(&self) -> io::Result<Vec<String>> {
        if self.storage_kind()? == RefStorageKind::Reftable {
            return Ok(self
                .read_reftable_refs()?
                .into_keys()
                .filter(|name| is_migratable_root_ref_name(name))
                .collect());
        }

        let mut refs = BTreeSet::new();
        for entry in fs::read_dir(self.storage_root()?)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_migratable_root_ref_name(&name) {
                continue;
            }
            if self.read_ref(&name).is_ok() {
                refs.insert(name);
            }
        }
        Ok(refs.into_iter().collect())
    }

    pub fn for_each_ref_name<E, F>(&self, prefix: &str, mut on_ref: F) -> std::result::Result<(), E>
    where
        E: From<io::Error>,
        F: FnMut(&str) -> std::result::Result<(), E>,
    {
        validate_ref_prefix(prefix).map_err(E::from)?;
        if self.storage_kind().map_err(E::from)? == RefStorageKind::Reftable {
            let refs = self.read_reftable_refs().map_err(E::from)?;
            for name in refs.keys().filter(|name| name.starts_with(prefix)) {
                on_ref(name)?;
            }
            return Ok(());
        }
        let mut refs = BTreeSet::new();
        collect_loose_refs(self.storage_root().map_err(E::from)?, prefix, &mut refs)
            .map_err(E::from)?;
        self.for_each_packed_ref(|_, name| {
            if name.starts_with(prefix) {
                refs.insert(name.to_owned());
            }
            Ok(true)
        })
        .map_err(E::from)?;
        for name in refs {
            on_ref(&name)?;
        }
        Ok(())
    }

    pub fn for_each_resolved_ref<E, F>(
        &self,
        prefix: &str,
        mut on_ref: F,
    ) -> std::result::Result<(), E>
    where
        E: From<io::Error>,
        F: FnMut(&str, &ObjectId) -> std::result::Result<(), E>,
    {
        for (name, id) in self.resolved_refs(prefix).map_err(E::from)? {
            on_ref(&name, &id)?;
        }
        Ok(())
    }

    pub fn pack_refs(&self, options: PackRefsOptions) -> io::Result<()> {
        if self.storage_kind()? == RefStorageKind::Reftable {
            if options.auto {
                return auto_compact_reftable_stack(self.storage_root()?, self.algorithm);
            }
            return compact_reftable_stack(self.storage_root()?, self.algorithm);
        }
        if options.auto && !self.should_auto_pack_refs(&options)? {
            return Ok(());
        }
        let packed_refs = self.read_packed_refs()?;
        let packed_names = packed_refs.keys().cloned().collect::<BTreeSet<_>>();
        let mut refs = packed_refs;
        let mut prune_names = Vec::new();
        let use_include_patterns = !options.all && !options.include.is_empty();

        for name in self.loose_ref_names("refs/")? {
            let excluded = options
                .exclude
                .iter()
                .any(|pattern| Self::ref_glob_matches(&name, pattern));
            let included = if use_include_patterns {
                options
                    .include
                    .iter()
                    .any(|pattern| Self::ref_glob_matches(&name, pattern))
            } else {
                false
            };
            let should_pack = if options.all {
                !excluded
            } else if use_include_patterns {
                included && !excluded
            } else {
                (name.starts_with("refs/tags/") || packed_names.contains(&name)) && !excluded
            };
            if !should_pack || !should_pack_ref_name(&name) {
                continue;
            }
            if let RefTarget::Direct(id) = self.read_ref(&name)? {
                refs.insert(name.clone(), id);
                prune_names.push(name);
            }
        }

        let object_store = LooseObjectStore::new(self.git_dir.join("objects"), self.algorithm);
        let mut out = String::from("# pack-refs with: peeled fully-peeled sorted \n");
        for (name, id) in &refs {
            out.push_str(&id.to_hex());
            out.push(' ');
            out.push_str(name);
            out.push('\n');
            if let Some(peeled) = peel_tag_ref(&object_store, id)? {
                out.push('^');
                out.push_str(&peeled.to_hex());
                out.push('\n');
            }
        }
        write_packed_refs_file(self.storage_root()?, out.as_bytes())?;

        if options.prune {
            for name in prune_names {
                let path = self.ref_path(&name);
                match fs::remove_file(&path) {
                    Ok(()) => {
                        prune_empty_ref_parent_dirs(self.storage_root()?, &path)?;
                    }
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err),
                }
            }
        }
        Ok(())
    }

    fn should_auto_pack_refs(&self, options: &PackRefsOptions) -> io::Result<bool> {
        let packed_count = self.read_packed_refs()?.len();
        let loose_count = self
            .loose_ref_names("refs/")?
            .into_iter()
            .filter(|name| should_pack_ref_name(name))
            .filter(|name| {
                !options
                    .exclude
                    .iter()
                    .any(|pattern| Self::ref_glob_matches(name, pattern))
            })
            .count();
        let threshold = std::cmp::max(16, packed_count.div_ceil(4));
        Ok(loose_count >= threshold)
    }

    fn ref_glob_matches(name: &str, pattern: &str) -> bool {
        let name = name.as_bytes();
        let pattern = pattern.as_bytes();
        let (mut name_idx, mut pattern_idx) = (0usize, 0usize);
        let (mut star_pattern, mut star_name) = (None, 0usize);

        while name_idx < name.len() {
            if pattern_idx < pattern.len() && pattern[pattern_idx] == name[name_idx] {
                name_idx += 1;
                pattern_idx += 1;
                continue;
            }
            if pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
                star_pattern = Some(pattern_idx);
                pattern_idx += 1;
                star_name = name_idx;
                continue;
            }
            if let Some(star_idx) = star_pattern {
                pattern_idx = star_idx + 1;
                star_name += 1;
                name_idx = star_name;
                continue;
            }
            return false;
        }

        while pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
            pattern_idx += 1;
        }
        pattern_idx == pattern.len()
    }

    pub fn write_fresh_packed_refs(
        &self,
        direct_refs: &[(String, ObjectId)],
        symbolic_refs: &[(String, String)],
    ) -> io::Result<()> {
        self.write_fresh_refs_with_logs(direct_refs, symbolic_refs, &[])
    }

    pub fn write_fresh_refs_with_logs(
        &self,
        direct_refs: &[(String, ObjectId)],
        symbolic_refs: &[(String, String)],
        logs: &[ReftableLogRecord],
    ) -> io::Result<()> {
        if self.storage_kind()? == RefStorageKind::Reftable {
            let mut refs = BTreeMap::new();
            for (name, id) in direct_refs {
                validate_storable_ref_name(name)?;
                if id.algorithm() != self.algorithm {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "object id algorithm does not match ref store",
                    ));
                }
                refs.insert(name.clone(), RefTarget::Direct(id.clone()));
            }
            for (name, target) in symbolic_refs {
                validate_storable_ref_name(name)?;
                validate_storable_ref_name(target)?;
                refs.insert(name.clone(), RefTarget::Symbolic(target.clone()));
            }
            return write_reftable_stack_with_logs(
                self.storage_root()?,
                self.algorithm,
                &refs,
                logs,
            );
        }
        let mut packed = BTreeMap::new();
        let mut direct_head = None;
        let mut loose_root_refs = Vec::new();
        for (name, id) in direct_refs {
            if name == "HEAD" {
                direct_head = Some(id.clone());
                continue;
            }
            validate_storable_ref_name(name)?;
            if id.algorithm() != self.algorithm {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "object id algorithm does not match ref store",
                ));
            }
            if name.starts_with("refs/") {
                packed.insert(name.clone(), id.clone());
            } else {
                loose_root_refs.push((name, id));
            }
        }
        for (name, target) in symbolic_refs {
            validate_storable_ref_name(name)?;
            validate_storable_ref_name(target)?;
        }

        if !packed.is_empty() {
            let mut out = String::from("# pack-refs with: sorted\n");
            for (name, id) in packed {
                out.push_str(&id.to_hex());
                out.push(' ');
                out.push_str(&name);
                out.push('\n');
            }
            atomic_write(self.storage_root()?.join("packed-refs"), out.as_bytes())?;
        }

        for (name, target) in symbolic_refs {
            self.write_symbolic_ref(name, target)?;
        }
        for (name, id) in loose_root_refs {
            self.write_ref(name, id)?;
        }
        if let Some(id) = direct_head {
            self.write_head_direct(&id)?;
        }
        Ok(())
    }

    pub fn server_info_refs(&self) -> io::Result<Vec<ServerInfoRef>> {
        let mut rows = Vec::new();
        self.for_each_server_info_ref(|id, name| {
            rows.push(ServerInfoRef {
                id: id.clone(),
                name: name.to_owned(),
            });
            Ok::<(), io::Error>(())
        })?;
        Ok(rows)
    }

    pub fn for_each_server_info_ref<E, F>(&self, mut on_ref: F) -> std::result::Result<(), E>
    where
        E: From<io::Error>,
        F: FnMut(&ObjectId, &str) -> std::result::Result<(), E>,
    {
        let store = LooseObjectStore::new(self.git_dir.join("objects"), self.algorithm);
        self.for_each_ref_name::<E, _>("refs/", |name| -> std::result::Result<(), E> {
            let id = match self.resolve(name) {
                Ok(id) => id,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound
                            | io::ErrorKind::NotADirectory
                            | io::ErrorKind::IsADirectory
                    ) =>
                {
                    return Ok(());
                }
                Err(error) => return Err(E::from(error)),
            };
            on_ref(&id, name)?;
            if let Some(peeled) = peel_tag_ref(&store, &id).map_err(E::from)? {
                on_ref(&peeled, &format!("{name}^{{}}"))?;
            }
            Ok(())
        })?;
        Ok(())
    }

    pub fn write_head_symbolic(&self, target: &str) -> io::Result<()> {
        validate_ref_name(target)?;
        if self.storage_kind()? == RefStorageKind::Reftable {
            self.write_reftable_ref_update(
                "HEAD",
                Some(RefTarget::Symbolic(target.to_owned())),
                false,
            )?;
            return Ok(());
        }
        atomic_write(
            self.storage_root()?.join("HEAD"),
            format!("ref: {target}\n").as_bytes(),
        )
    }

    pub fn write_head_direct(&self, id: &ObjectId) -> io::Result<()> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match ref store",
            ));
        }
        if self.storage_kind()? == RefStorageKind::Reftable {
            self.write_reftable_ref_update("HEAD", Some(RefTarget::Direct(id.clone())), false)?;
            return Ok(());
        }
        atomic_write(
            self.storage_root()?.join("HEAD"),
            format!("{}\n", id.to_hex()).as_bytes(),
        )
    }

    pub fn read_head(&self) -> io::Result<RefTarget> {
        if self.storage_kind()? == RefStorageKind::Reftable
            && let Some(target) = self.read_reftable_ref("HEAD")?
        {
            return Ok(target);
        }
        let head_path = self.storage_root()?.join("HEAD");
        let target = match fs::symlink_metadata(&head_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                parse_symbolic_link_ref_target(&head_path, false)
            }
            Ok(_) => {
                let raw = fs::read_to_string(&head_path)?;
                parse_ref_target(self.algorithm, raw.trim_end_matches('\n'), false)
            }
            Err(error) => Err(error),
        };
        match target {
            Ok(target) => Ok(target),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData
                ) =>
            {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "reference broken",
                ))
            }
            Err(error) => Err(error),
        }
    }

    pub fn resolve(&self, name: &str) -> io::Result<ObjectId> {
        let mut seen = BTreeSet::new();
        self.resolve_symbolic(name, &mut seen)
    }

    fn resolve_symbolic(&self, name: &str, seen: &mut BTreeSet<String>) -> io::Result<ObjectId> {
        if !seen.insert(name.to_owned()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "symbolic ref cycle detected",
            ));
        }
        match if name == "HEAD" {
            self.read_head()?
        } else {
            self.read_ref(name)?
        } {
            RefTarget::Direct(id) => Ok(id),
            RefTarget::Symbolic(target) => {
                validate_storable_ref_name(&target).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "symbolic ref target is invalid")
                })?;
                self.resolve_symbolic(&target, seen)
            }
        }
    }

    fn ref_path(&self, name: &str) -> PathBuf {
        self.storage_root()
            .unwrap_or(self.git_dir.as_path())
            .join(name)
    }

    fn ensure_no_ref_name_conflict(&self, name: &str) -> io::Result<()> {
        let mut parent = name;
        while let Some((prefix, _)) = parent.rsplit_once('/') {
            if prefix == "refs" {
                break;
            }
            parent = prefix;
            match inspect_ref_path(&self.ref_path(parent), self.algorithm, true)? {
                RefPathState::Valid(_) | RefPathState::BrokenFile => {
                    return Err(ref_name_conflict_error(name, parent));
                }
                RefPathState::Missing | RefPathState::EmptyDir | RefPathState::BlockingDir => {}
            }
            if self.read_packed_ref(parent)?.is_some() {
                return Err(ref_name_conflict_error(name, parent));
            }
        }

        let descendant_prefix = format!("{name}/");
        let mut conflicting_descendant = None;
        self.for_each_packed_ref(|_, packed_name| {
            if packed_name.starts_with(&descendant_prefix) {
                conflicting_descendant = Some(packed_name.to_owned());
                return Ok(false);
            }
            Ok(true)
        })?;
        if let Some(conflicting_descendant) = conflicting_descendant {
            return Err(ref_name_conflict_error(name, &conflicting_descendant));
        }
        Ok(())
    }

    fn read_packed_ref(&self, name: &str) -> io::Result<Option<RefTarget>> {
        let mut found = None;
        self.for_each_packed_ref(|id, ref_name| {
            if ref_name == name {
                found = Some(RefTarget::Direct(id));
                return Ok(false);
            }
            Ok(true)
        })?;
        Ok(found)
    }

    fn read_packed_refs(&self) -> io::Result<BTreeMap<String, ObjectId>> {
        let mut refs = BTreeMap::new();
        self.for_each_packed_ref(|id, ref_name| {
            refs.insert(ref_name.to_owned(), id);
            Ok(true)
        })?;
        Ok(refs)
    }

    fn for_each_packed_ref<F>(&self, mut on_ref: F) -> io::Result<()>
    where
        F: FnMut(ObjectId, &str) -> io::Result<bool>,
    {
        for_each_packed_ref_line(self.storage_root()?, |line| {
            if let Some((id, ref_name)) = parse_packed_ref_line(self.algorithm, line)? {
                return on_ref(id, &ref_name);
            }
            Ok(true)
        })
    }

    pub fn resolved_refs(&self, prefix: &str) -> io::Result<BTreeMap<String, ObjectId>> {
        validate_ref_prefix(prefix)?;
        if self.storage_kind()? == RefStorageKind::Reftable {
            let targets = self.read_reftable_refs()?;
            let mut refs = BTreeMap::new();
            for name in targets.keys().filter(|name| name.starts_with(prefix)) {
                match resolve_reftable_target(&targets, name, &mut BTreeSet::new()) {
                    Ok(id) => {
                        refs.insert(name.clone(), id);
                    }
                    Err(error) if should_skip_resolved_ref_error(&error) => {}
                    Err(error) => return Err(error),
                }
            }
            return Ok(refs);
        }
        let mut refs = BTreeMap::new();
        self.for_each_packed_ref(|id, name| {
            if name.starts_with(prefix) {
                refs.insert(name.to_owned(), id);
            }
            Ok(true)
        })?;
        for name in self.loose_ref_names(prefix)? {
            match self.resolve(&name) {
                Ok(id) => {
                    refs.insert(name.clone(), id);
                }
                Err(error) if should_skip_resolved_ref_error(&error) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(refs)
    }

    fn loose_ref_names(&self, prefix: &str) -> io::Result<Vec<String>> {
        validate_ref_prefix(prefix)?;
        let mut refs = BTreeSet::new();
        collect_loose_refs(self.storage_root()?, prefix, &mut refs)?;
        Ok(refs.into_iter().collect())
    }

    fn read_reftable_ref(&self, name: &str) -> io::Result<Option<RefTarget>> {
        Ok(self.read_reftable_refs()?.remove(name))
    }

    fn read_reftable_refs(&self) -> io::Result<BTreeMap<String, RefTarget>> {
        read_reftable_stack(self.storage_root()?, self.algorithm)
    }

    fn write_reftable_ref_update(
        &self,
        name: &str,
        target: Option<RefTarget>,
        check_name_conflict: bool,
    ) -> io::Result<bool> {
        let git_dir = self.storage_root()?;
        let reftable_dir = git_dir.join("reftable");
        fs::create_dir_all(&reftable_dir)?;
        let lock = acquire_reftable_stack_lock(&reftable_dir)?;
        let current = read_reftable_stack(git_dir, self.algorithm).or_else(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                Ok(BTreeMap::new())
            } else {
                Err(error)
            }
        })?;
        if target.is_some() && check_name_conflict {
            ensure_no_reftable_ref_name_conflict(&current, name)?;
        }
        let existed = current.contains_key(name);
        if current.get(name) == target.as_ref() {
            return Ok(existed);
        }
        let mut updates = BTreeMap::new();
        updates.insert(name.to_owned(), target);
        append_reftable_delta_with_lock(git_dir, self.algorithm, &updates, &[], lock)?;
        Ok(existed)
    }

    pub fn reftable_logs(&self) -> io::Result<Vec<ReftableLogRecord>> {
        Ok(self
            .all_reftable_logs()?
            .into_iter()
            .filter(|record| !reftable_log_record_is_existence_marker(record))
            .collect())
    }

    fn all_reftable_logs(&self) -> io::Result<Vec<ReftableLogRecord>> {
        if self.storage_kind()? != RefStorageKind::Reftable {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reftable logs require the reftable ref backend",
            ));
        }
        read_reftable_log_stack(self.storage_root()?, self.algorithm)
    }

    pub fn append_reftable_log(&self, mut record: ReftableLogRecord) -> io::Result<()> {
        if self.storage_kind()? != RefStorageKind::Reftable {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reftable logs require the reftable ref backend",
            ));
        }
        let git_dir = self.storage_root()?;
        let reftable_dir = git_dir.join("reftable");
        fs::create_dir_all(&reftable_dir)?;
        let lock = acquire_reftable_stack_lock(&reftable_dir)?;
        record.update_index = next_reftable_update_index(&reftable_dir)?;
        append_reftable_delta_with_lock(
            git_dir,
            self.algorithm,
            &BTreeMap::new(),
            &[ParsedReftableLogRecord::Update(record)],
            lock,
        )
    }

    pub fn write_reftable_ref_with_logs(
        &self,
        name: &str,
        target: RefTarget,
        mut logs: Vec<ReftableLogRecord>,
    ) -> io::Result<()> {
        if self.storage_kind()? != RefStorageKind::Reftable {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "combined ref and log writes require the reftable backend",
            ));
        }
        validate_storable_ref_name(name)?;
        let git_dir = self.storage_root()?;
        let reftable_dir = git_dir.join("reftable");
        fs::create_dir_all(&reftable_dir)?;
        let lock = acquire_reftable_stack_lock(&reftable_dir)?;
        let current = read_reftable_stack(git_dir, self.algorithm)?;
        ensure_no_reftable_ref_name_conflict(&current, name)?;
        let update_index = next_reftable_update_index(&reftable_dir)?;
        for log in &mut logs {
            log.update_index = update_index;
        }
        let updates = BTreeMap::from([(name.to_owned(), Some(target))]);
        let logs = logs
            .into_iter()
            .map(ParsedReftableLogRecord::Update)
            .collect::<Vec<_>>();
        append_reftable_delta_with_lock(git_dir, self.algorithm, &updates, &logs, lock)
    }

    pub fn delete_reftable_ref_with_log(&self, name: &str) -> io::Result<bool> {
        if self.storage_kind()? != RefStorageKind::Reftable {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "combined ref and log deletes require the reftable backend",
            ));
        }
        validate_storable_ref_name(name)?;
        let git_dir = self.storage_root()?;
        let reftable_dir = git_dir.join("reftable");
        let lock = acquire_reftable_stack_lock(&reftable_dir)?;
        let existed = read_reftable_stack(git_dir, self.algorithm)?.contains_key(name);
        if !existed {
            return Ok(false);
        }
        let updates = BTreeMap::from([(name.to_owned(), None)]);
        let logs = self
            .all_reftable_logs()?
            .into_iter()
            .filter(|record| record.ref_name == name)
            .map(|record| ParsedReftableLogRecord::Deletion {
                ref_name: name.to_owned(),
                update_index: record.update_index,
            })
            .collect::<Vec<_>>();
        append_reftable_delta_with_lock(git_dir, self.algorithm, &updates, &logs, lock)?;
        Ok(true)
    }

    pub fn reftable_log_exists(&self, ref_name: &str) -> io::Result<bool> {
        if self.storage_kind()? != RefStorageKind::Reftable {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reftable logs require the reftable ref backend",
            ));
        }
        Ok(self
            .all_reftable_logs()?
            .iter()
            .any(|record| record.ref_name == ref_name))
    }

    pub fn create_reftable_log(&self, ref_name: &str) -> io::Result<()> {
        validate_storable_ref_name(ref_name)?;
        if self.reftable_log_exists(ref_name)? {
            return Ok(());
        }
        let zero = reftable_zero_object_id(self.algorithm);
        self.append_reftable_log(ReftableLogRecord {
            ref_name: ref_name.to_owned(),
            update_index: 0,
            old_id: zero.clone(),
            new_id: zero,
            name: String::new(),
            email: String::new(),
            timestamp: 0,
            timezone_offset: 0,
            message: String::new(),
        })
    }

    pub fn delete_reftable_log(&self, ref_name: &str) -> io::Result<bool> {
        let indices = self
            .all_reftable_logs()?
            .into_iter()
            .filter(|record| record.ref_name == ref_name)
            .map(|record| record.update_index)
            .collect::<Vec<_>>();
        if indices.is_empty() {
            return Ok(false);
        }
        self.delete_reftable_log_entries(ref_name, &indices, false)?;
        Ok(true)
    }

    pub fn delete_reftable_log_entries(
        &self,
        ref_name: &str,
        update_indices: &[u64],
        preserve_existence: bool,
    ) -> io::Result<()> {
        if self.storage_kind()? != RefStorageKind::Reftable {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reftable logs require the reftable ref backend",
            ));
        }
        validate_storable_ref_name(ref_name)?;
        if update_indices.is_empty() {
            return Ok(());
        }
        let git_dir = self.storage_root()?;
        let reftable_dir = git_dir.join("reftable");
        let lock = acquire_reftable_stack_lock(&reftable_dir)?;
        let mut records = update_indices
            .iter()
            .map(|update_index| ParsedReftableLogRecord::Deletion {
                ref_name: ref_name.to_owned(),
                update_index: *update_index,
            })
            .collect::<Vec<_>>();
        if preserve_existence {
            let deleting = update_indices.iter().copied().collect::<BTreeSet<_>>();
            let has_remaining = self.all_reftable_logs()?.iter().any(|record| {
                record.ref_name == ref_name && !deleting.contains(&record.update_index)
            });
            if !has_remaining {
                let update_index = next_reftable_update_index(&reftable_dir)?;
                let zero = reftable_zero_object_id(self.algorithm);
                records.push(ParsedReftableLogRecord::Update(ReftableLogRecord {
                    ref_name: ref_name.to_owned(),
                    update_index,
                    old_id: zero.clone(),
                    new_id: zero,
                    name: String::new(),
                    email: String::new(),
                    timestamp: 0,
                    timezone_offset: 0,
                    message: String::new(),
                }));
            }
        }
        append_reftable_delta_with_lock(git_dir, self.algorithm, &BTreeMap::new(), &records, lock)
    }

    pub fn copy_reftable_log(
        &self,
        old_name: &str,
        new_name: &str,
        delete_old: bool,
    ) -> io::Result<()> {
        validate_storable_ref_name(old_name)?;
        validate_storable_ref_name(new_name)?;
        let source = self
            .all_reftable_logs()?
            .into_iter()
            .filter(|record| record.ref_name == old_name)
            .collect::<Vec<_>>();
        if source.is_empty() {
            return Ok(());
        }
        let git_dir = self.storage_root()?;
        let reftable_dir = git_dir.join("reftable");
        let lock = acquire_reftable_stack_lock(&reftable_dir)?;
        let mut records = Vec::with_capacity(source.len() * if delete_old { 2 } else { 1 });
        for record in source {
            if delete_old {
                records.push(ParsedReftableLogRecord::Deletion {
                    ref_name: old_name.to_owned(),
                    update_index: record.update_index,
                });
            }
            records.push(ParsedReftableLogRecord::Update(ReftableLogRecord {
                ref_name: new_name.to_owned(),
                ..record
            }));
        }
        append_reftable_delta_with_lock(git_dir, self.algorithm, &BTreeMap::new(), &records, lock)
    }
}

fn reftable_zero_object_id(algorithm: GitHashAlgorithm) -> ObjectId {
    match algorithm {
        GitHashAlgorithm::Sha1 => ObjectId::new(algorithm, &[0; 20]),
        GitHashAlgorithm::Sha256 => ObjectId::new(algorithm, &[0; 32]),
    }
}

fn reftable_log_record_is_existence_marker(record: &ReftableLogRecord) -> bool {
    record.old_id.as_bytes().iter().all(|byte| *byte == 0)
        && record.new_id.as_bytes().iter().all(|byte| *byte == 0)
}

fn resolve_reftable_target(
    refs: &BTreeMap<String, RefTarget>,
    name: &str,
    seen: &mut BTreeSet<String>,
) -> io::Result<ObjectId> {
    if !seen.insert(name.to_owned()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "symbolic ref cycle detected",
        ));
    }
    match refs.get(name) {
        Some(RefTarget::Direct(id)) => Ok(id.clone()),
        Some(RefTarget::Symbolic(target)) => resolve_reftable_target(refs, target, seen),
        None => Err(io::Error::new(io::ErrorKind::NotFound, "ref not found")),
    }
}

fn ref_name_conflict_error(name: &str, existing: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("'{existing}' exists; cannot create '{name}'"),
    )
}

fn ensure_no_reftable_ref_name_conflict(
    refs: &BTreeMap<String, RefTarget>,
    name: &str,
) -> io::Result<()> {
    let descendant_prefix = format!("{name}/");
    if let Some(existing) = refs.keys().find(|existing| {
        existing.as_str() != name
            && (name.starts_with(&format!("{existing}/"))
                || existing.starts_with(&descendant_prefix))
    }) {
        return Err(ref_name_conflict_error(name, existing));
    }
    Ok(())
}

pub fn initialize_reftable_ref_store(
    git_dir: &Path,
    algorithm: GitHashAlgorithm,
    head_target: &str,
) -> io::Result<()> {
    validate_ref_name(head_target)?;
    let mut refs = BTreeMap::new();
    refs.insert(
        "HEAD".to_owned(),
        RefTarget::Symbolic(head_target.to_owned()),
    );
    write_reftable_stack(git_dir, algorithm, &refs)
}

pub fn initialize_alternate_ref_store(
    git_dir: &Path,
    storage_root: &Path,
    algorithm: GitHashAlgorithm,
    kind: RefStorageKind,
    head_target: &str,
) -> io::Result<()> {
    if head_target != "refs/heads/.invalid" {
        validate_ref_name(head_target)?;
    }
    fs::create_dir_all(storage_root)?;
    match kind {
        RefStorageKind::Files => {
            fs::create_dir_all(storage_root.join("refs/heads"))?;
            atomic_write(
                storage_root.join("HEAD"),
                format!("ref: {head_target}\n").as_bytes(),
            )?;
        }
        RefStorageKind::Reftable => {
            let mut refs = BTreeMap::new();
            refs.insert(
                "HEAD".to_owned(),
                RefTarget::Symbolic(head_target.to_owned()),
            );
            write_reftable_stack(storage_root, algorithm, &refs)?;
        }
    }
    let refs_dir = git_dir.join("refs");
    fs::create_dir_all(&refs_dir)?;
    let heads = refs_dir.join("heads");
    if heads.is_dir() {
        fs::remove_dir_all(&heads)?;
    }
    fs::write(heads, "repository uses alternate refs storage\n")?;
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/.invalid\n")
}

fn peel_tag_ref<S: GitObjectStore>(store: &S, id: &ObjectId) -> io::Result<Option<ObjectId>> {
    let object = match store.read_object(id) {
        Ok(object) => object,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if object.kind != GitObjectKind::Tag {
        return Ok(None);
    }
    let mut tag = decode_tag(id.algorithm(), &object.content)?;
    loop {
        let next = match store.read_object(&tag.target) {
            Ok(object) => object,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if next.kind != tag.target_kind {
            return Ok(None);
        }
        if tag.target_kind != GitObjectKind::Tag {
            return Ok(Some(tag.target));
        }
        tag = decode_tag(id.algorithm(), &next.content)?;
    }
}

fn parse_ref_target(
    algorithm: GitHashAlgorithm,
    value: &str,
    allow_onelevel_symbolic_target: bool,
) -> io::Result<RefTarget> {
    if let Some(target) = value.strip_prefix("ref: ") {
        validate_ref_format(target, allow_onelevel_symbolic_target)?;
        return Ok(RefTarget::Symbolic(target.to_string()));
    }
    ObjectId::from_hex(algorithm, value).map(RefTarget::Direct)
}

fn parse_symbolic_link_ref_target(
    path: &Path,
    allow_onelevel_symbolic_target: bool,
) -> io::Result<RefTarget> {
    let target = fs::read_link(path)?;
    let target = target
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "reference broken"))?;
    validate_ref_format(target, allow_onelevel_symbolic_target)?;
    Ok(RefTarget::Symbolic(target.to_owned()))
}

fn should_pack_ref_name(name: &str) -> bool {
    !name.starts_with("refs/bisect/") && !name.starts_with("refs/worktree/")
}

fn read_reftable_stack(
    git_dir: &Path,
    algorithm: GitHashAlgorithm,
) -> io::Result<BTreeMap<String, RefTarget>> {
    let reftable_dir = git_dir.join("reftable");
    read_consistent_reftable_stack(&reftable_dir, |tables| {
        let mut refs = BTreeMap::new();
        for table in tables.lines() {
            let table = table.trim();
            if table.is_empty() {
                continue;
            }
            for (name, target) in read_reftable_file(&reftable_dir.join(table), algorithm)? {
                if let Some(target) = target {
                    refs.insert(name, target);
                } else {
                    refs.remove(&name);
                }
            }
        }
        Ok(refs)
    })
}

#[derive(Debug, Clone)]
enum ParsedReftableLogRecord {
    Update(ReftableLogRecord),
    Deletion { ref_name: String, update_index: u64 },
}

impl ParsedReftableLogRecord {
    fn key(&self) -> (String, u64) {
        match self {
            Self::Update(record) => (record.ref_name.clone(), record.update_index),
            Self::Deletion {
                ref_name,
                update_index,
            } => (ref_name.clone(), *update_index),
        }
    }
}

fn read_reftable_log_stack(
    git_dir: &Path,
    algorithm: GitHashAlgorithm,
) -> io::Result<Vec<ReftableLogRecord>> {
    let reftable_dir = git_dir.join("reftable");
    read_consistent_reftable_stack(&reftable_dir, |tables| {
        let mut logs = BTreeMap::new();
        for table in tables.lines() {
            let table = table.trim();
            if table.is_empty() {
                continue;
            }
            for record in read_reftable_log_file(&reftable_dir.join(table), algorithm)? {
                let key = record.key();
                match record {
                    ParsedReftableLogRecord::Update(record) => {
                        logs.insert(key, Some(record));
                    }
                    ParsedReftableLogRecord::Deletion { .. } => {
                        logs.insert(key, None);
                    }
                }
            }
        }
        Ok(logs.into_values().flatten().collect())
    })
}

fn read_consistent_reftable_stack<T>(
    reftable_dir: &Path,
    mut read_tables: impl FnMut(&str) -> io::Result<T>,
) -> io::Result<T> {
    let table_list_path = reftable_dir.join("tables.list");
    loop {
        let before = fs::read_to_string(&table_list_path)?;
        match read_tables(&before) {
            Ok(value) => {
                if fs::read_to_string(&table_list_path)? == before {
                    return Ok(value);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if fs::read_to_string(&table_list_path)? == before {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
        thread::yield_now();
    }
}

fn read_reftable_log_file(
    path: &Path,
    algorithm: GitHashAlgorithm,
) -> io::Result<Vec<ParsedReftableLogRecord>> {
    let bytes = fs::read(path)?;
    let header = parse_reftable_header(&bytes, algorithm)?;
    let footer_len = match algorithm {
        GitHashAlgorithm::Sha1 => REFTABLE_FOOTER_V1_LEN,
        GitHashAlgorithm::Sha256 => REFTABLE_FOOTER_V2_LEN,
    };
    let footer_start = bytes
        .len()
        .checked_sub(footer_len)
        .ok_or_else(|| reftable_invalid("truncated reftable footer"))?;
    let footer_log_offset = header
        .header_len
        .checked_add(24)
        .ok_or_else(|| reftable_invalid("reftable footer offset overflow"))?;
    let log_position = usize::try_from(read_u64(&bytes[footer_start..], footer_log_offset)?)
        .map_err(|_| reftable_invalid("reftable log position overflow"))?;
    let first_block_is_log = bytes.get(header.header_len).copied() == Some(b'g');
    if log_position == 0 && !first_block_is_log {
        return Ok(Vec::new());
    }
    let mut block_start = log_position;
    let mut records = Vec::new();
    while block_start < footer_start {
        let header_offset = if block_start == 0 {
            header.header_len
        } else {
            0
        };
        let block_type_offset = block_start
            .checked_add(header_offset)
            .ok_or_else(|| reftable_invalid("reftable log block offset overflow"))?;
        if bytes.get(block_type_offset).copied() != Some(b'g') {
            break;
        }
        let block_len = read_u24(&bytes, block_type_offset + 1)?;
        let block_header_len = header_offset + REFTABLE_BLOCK_HEADER_LEN;
        let body_len = block_len
            .checked_sub(block_header_len)
            .ok_or_else(|| reftable_invalid("invalid reftable log block length"))?;
        let compressed_start = block_start
            .checked_add(block_header_len)
            .ok_or_else(|| reftable_invalid("reftable log data offset overflow"))?;
        if compressed_start > footer_start {
            return Err(reftable_invalid("truncated reftable log block"));
        }
        let mut decoder = ZlibDecoder::new(&bytes[compressed_start..footer_start]);
        let mut body = Vec::with_capacity(body_len);
        decoder.read_to_end(&mut body)?;
        if body.len() != body_len {
            return Err(reftable_invalid(
                "invalid inflated reftable log block length",
            ));
        }
        let consumed = usize::try_from(decoder.total_in())
            .map_err(|_| reftable_invalid("reftable compressed block length overflow"))?;
        parse_reftable_log_block(&body, algorithm, &mut records)?;
        block_start = compressed_start
            .checked_add(consumed)
            .ok_or_else(|| reftable_invalid("reftable log block offset overflow"))?;
    }
    Ok(records)
}

fn parse_reftable_log_block(
    body: &[u8],
    algorithm: GitHashAlgorithm,
    records: &mut Vec<ParsedReftableLogRecord>,
) -> io::Result<()> {
    let restart_count_offset = body
        .len()
        .checked_sub(REFTABLE_RESTART_COUNT_LEN)
        .ok_or_else(|| reftable_invalid("truncated reftable log restart count"))?;
    let restart_count =
        u16::from_be_bytes([body[restart_count_offset], body[restart_count_offset + 1]]) as usize;
    let record_end = restart_count_offset
        .checked_sub(
            restart_count
                .checked_mul(3)
                .ok_or_else(|| reftable_invalid("reftable log restart table overflow"))?,
        )
        .ok_or_else(|| reftable_invalid("truncated reftable log restart table"))?;
    let mut cursor = 0;
    let mut prior_key = Vec::new();
    while cursor < record_end {
        let prefix_len = read_reftable_varint(body, &mut cursor)? as usize;
        let suffix_and_type = read_reftable_varint(body, &mut cursor)?;
        let suffix_len = (suffix_and_type >> 3) as usize;
        let value_type = (suffix_and_type & 0x7) as u8;
        if prefix_len > prior_key.len() || cursor + suffix_len > record_end {
            return Err(reftable_invalid("invalid reftable log key"));
        }
        let mut key = prior_key[..prefix_len].to_vec();
        key.extend_from_slice(&body[cursor..cursor + suffix_len]);
        cursor += suffix_len;
        let (ref_name, update_index) = parse_reftable_log_key(&key)?;
        let parsed = match value_type {
            0 => ParsedReftableLogRecord::Deletion {
                ref_name,
                update_index,
            },
            1 => {
                let old_id = read_reftable_object_id(body, &mut cursor, algorithm, record_end)?;
                let new_id = read_reftable_object_id(body, &mut cursor, algorithm, record_end)?;
                let name = read_reftable_string(body, &mut cursor, record_end)?;
                let email = read_reftable_string(body, &mut cursor, record_end)?;
                let timestamp = read_reftable_varint(body, &mut cursor)?;
                if cursor + 2 > record_end {
                    return Err(reftable_invalid("truncated reftable log timezone"));
                }
                let timezone_offset = i16::from_be_bytes([body[cursor], body[cursor + 1]]);
                cursor += 2;
                let message = read_reftable_string(body, &mut cursor, record_end)?;
                ParsedReftableLogRecord::Update(ReftableLogRecord {
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
            _ => return Err(reftable_invalid("unsupported reftable log value type")),
        };
        prior_key = key;
        records.push(parsed);
    }
    Ok(())
}

fn parse_reftable_log_key(key: &[u8]) -> io::Result<(String, u64)> {
    if key.len() <= 9 || key[key.len() - 9] != 0 {
        return Err(reftable_invalid("invalid reftable log key"));
    }
    let name_end = key.len() - 9;
    let ref_name = String::from_utf8(key[..name_end].to_vec())
        .map_err(|_| reftable_invalid("non-utf8 reftable log ref name"))?;
    validate_storable_ref_name(&ref_name)?;
    let encoded = u64::from_be_bytes(
        key[key.len() - 8..]
            .try_into()
            .expect("reftable log key length checked"),
    );
    Ok((ref_name, u64::MAX - encoded))
}

fn read_reftable_string(bytes: &[u8], cursor: &mut usize, record_end: usize) -> io::Result<String> {
    let len = read_reftable_varint(bytes, cursor)? as usize;
    if *cursor + len > record_end {
        return Err(reftable_invalid("truncated reftable string"));
    }
    let value = String::from_utf8(bytes[*cursor..*cursor + len].to_vec())
        .map_err(|_| reftable_invalid("non-utf8 reftable string"))?;
    *cursor += len;
    Ok(value)
}

fn write_reftable_stack(
    git_dir: &Path,
    algorithm: GitHashAlgorithm,
    refs: &BTreeMap<String, RefTarget>,
) -> io::Result<()> {
    write_reftable_stack_with_logs(git_dir, algorithm, refs, &[])
}

fn write_reftable_stack_with_logs(
    git_dir: &Path,
    algorithm: GitHashAlgorithm,
    refs: &BTreeMap<String, RefTarget>,
    logs: &[ReftableLogRecord],
) -> io::Result<()> {
    let reftable_dir = git_dir.join("reftable");
    fs::create_dir_all(&reftable_dir)?;
    write_reftable_dummy_files(git_dir)?;
    let lock = acquire_reftable_stack_lock(&reftable_dir)?;
    write_reftable_stack_with_logs_locked(git_dir, algorithm, refs, logs, lock)
}

fn write_reftable_stack_with_logs_locked(
    git_dir: &Path,
    algorithm: GitHashAlgorithm,
    refs: &BTreeMap<String, RefTarget>,
    logs: &[ReftableLogRecord],
    lock: ReftableStackLock,
) -> io::Result<()> {
    let parsed_logs = logs
        .iter()
        .cloned()
        .map(ParsedReftableLogRecord::Update)
        .collect::<Vec<_>>();
    let reftable_dir = git_dir.join("reftable");
    let old_tables = fs::read_to_string(reftable_dir.join("tables.list")).unwrap_or_default();
    let next_update_index = next_reftable_update_index(&reftable_dir)?;
    let latest_update_index = next_update_index.saturating_sub(1);
    let min_update_index = logs
        .iter()
        .map(|record| record.update_index)
        .min()
        .unwrap_or(next_update_index);
    let max_update_index = logs
        .iter()
        .map(|record| record.update_index)
        .max()
        .unwrap_or(next_update_index)
        .max(latest_update_index)
        .max(min_update_index);
    let table_name = reftable_table_name(min_update_index, max_update_index);
    let table_path = reftable_dir.join(&table_name);
    let updates = refs
        .iter()
        .map(|(name, target)| (name.clone(), Some(target.clone())))
        .collect::<BTreeMap<_, _>>();
    let bytes = encode_reftable(
        min_update_index,
        max_update_index,
        &updates,
        &parsed_logs,
        algorithm,
        reftable_write_options(&reftable_dir)?,
    )?;
    atomic_write(table_path, &bytes)?;
    lock.commit(format!("{table_name}\n").as_bytes())?;
    for old_table in old_tables
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if old_table != table_name {
            match fs::remove_file(reftable_dir.join(old_table)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

fn append_reftable_delta_with_lock(
    git_dir: &Path,
    algorithm: GitHashAlgorithm,
    updates: &BTreeMap<String, Option<RefTarget>>,
    logs: &[ParsedReftableLogRecord],
    lock: ReftableStackLock,
) -> io::Result<()> {
    if updates.is_empty() && logs.is_empty() {
        return Ok(());
    }
    write_reftable_dummy_files(git_dir)?;
    let reftable_dir = git_dir.join("reftable");
    let next_update_index = next_reftable_update_index(&reftable_dir)?;
    let min_update_index = logs
        .iter()
        .map(|record| record.key().1)
        .min()
        .unwrap_or(next_update_index);
    let max_update_index = logs
        .iter()
        .map(|record| record.key().1)
        .max()
        .unwrap_or(next_update_index)
        .max(min_update_index);
    let table_name = reftable_table_name(min_update_index, max_update_index);
    let bytes = encode_reftable(
        min_update_index,
        max_update_index,
        updates,
        logs,
        algorithm,
        reftable_write_options(&reftable_dir)?,
    )?;
    atomic_write(reftable_dir.join(&table_name), &bytes)?;
    let mut tables = fs::read_to_string(reftable_dir.join("tables.list")).unwrap_or_default();
    if !tables.is_empty() && !tables.ends_with('\n') {
        tables.push('\n');
    }
    tables.push_str(&table_name);
    tables.push('\n');
    lock.commit(tables.as_bytes())?;
    if reftable_auto_compaction_enabled() {
        let _ = auto_compact_reftable_stack(git_dir, algorithm);
    }
    Ok(())
}

fn reftable_auto_compaction_enabled() -> bool {
    !std::env::var("GIT_TEST_REFTABLE_AUTOCOMPACTION")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
}

fn auto_compact_reftable_stack(git_dir: &Path, algorithm: GitHashAlgorithm) -> io::Result<()> {
    let reftable_dir = git_dir.join("reftable");
    let lock = acquire_reftable_stack_lock(&reftable_dir)?;
    let tables = fs::read_to_string(reftable_dir.join("tables.list"))?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if tables.len() == 2 && git_dir.join("commondir").is_file() {
        return Ok(());
    }
    let sizes = tables
        .iter()
        .map(|table| fs::metadata(reftable_dir.join(table)).map(|metadata| metadata.len()))
        .collect::<io::Result<Vec<_>>>()?;
    let Some((mut start, end)) =
        suggest_reftable_compaction_segment(&sizes, REFTABLE_DEFAULT_GEOMETRIC_FACTOR)
    else {
        return Ok(());
    };
    if let Some(locked) = (start..end)
        .rev()
        .find(|index| lock_path(&reftable_dir.join(&tables[*index])).exists())
    {
        start = locked + 1;
    }
    if end.saturating_sub(start) <= 1 {
        return Ok(());
    }
    compact_reftable_range_locked(git_dir, algorithm, &tables, start, end, lock)
}

fn suggest_reftable_compaction_segment(sizes: &[u64], factor: u64) -> Option<(usize, usize)> {
    if sizes.len() <= 1 {
        return None;
    }
    let mut end = None;
    let mut bytes = 0_u64;
    let mut index = sizes.len() - 1;
    while index > 0 {
        if sizes[index - 1] < sizes[index].saturating_mul(factor) {
            end = Some(index + 1);
            bytes = sizes[index];
            break;
        }
        index -= 1;
    }
    let end = end?;
    let mut start = 0;
    while index > 0 {
        let current = bytes;
        bytes = bytes.saturating_add(sizes[index - 1]);
        if sizes[index - 1] < current.saturating_mul(factor) {
            start = index - 1;
        }
        index -= 1;
    }
    Some((start, end))
}

fn compact_reftable_range_locked(
    git_dir: &Path,
    algorithm: GitHashAlgorithm,
    tables: &[String],
    start: usize,
    end: usize,
    lock: ReftableStackLock,
) -> io::Result<()> {
    let reftable_dir = git_dir.join("reftable");
    let selected = tables
        .get(start..end)
        .ok_or_else(|| reftable_invalid("invalid reftable compaction range"))?;
    let first = selected
        .first()
        .ok_or_else(|| reftable_invalid("empty reftable compaction range"))?;
    let last = selected
        .last()
        .ok_or_else(|| reftable_invalid("empty reftable compaction range"))?;
    let first_header = parse_reftable_header(&fs::read(reftable_dir.join(first))?, algorithm)?;
    let last_header = parse_reftable_header(&fs::read(reftable_dir.join(last))?, algorithm)?;
    let mut updates = BTreeMap::new();
    let mut logs = BTreeMap::new();
    for table in selected {
        let path = reftable_dir.join(table);
        for (name, target) in read_reftable_file(&path, algorithm)? {
            updates.insert(name, target);
        }
        for record in read_reftable_log_file(&path, algorithm)? {
            let key = record.key();
            logs.insert(key, record);
        }
    }
    if start == 0 {
        updates.retain(|_, target| target.is_some());
    }
    let logs = logs
        .into_values()
        .filter(|record| start != 0 || matches!(record, ParsedReftableLogRecord::Update(_)))
        .collect::<Vec<_>>();
    let table_name =
        reftable_table_name(first_header.min_update_index, last_header.max_update_index);
    let bytes = encode_reftable(
        first_header.min_update_index,
        last_header.max_update_index,
        &updates,
        &logs,
        algorithm,
        reftable_write_options(&reftable_dir)?,
    )?;
    atomic_write(reftable_dir.join(&table_name), &bytes)?;
    let mut new_tables = Vec::with_capacity(tables.len() - selected.len() + 1);
    new_tables.extend_from_slice(&tables[..start]);
    new_tables.push(table_name.clone());
    new_tables.extend_from_slice(&tables[end..]);
    let table_list = new_tables
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    lock.commit(table_list.as_bytes())?;
    for old_table in selected {
        if old_table != &table_name {
            match fs::remove_file(reftable_dir.join(old_table)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

fn compact_reftable_stack(git_dir: &Path, algorithm: GitHashAlgorithm) -> io::Result<()> {
    let reftable_dir = git_dir.join("reftable");
    let lock = acquire_reftable_stack_lock(&reftable_dir)?;
    let refs = read_reftable_stack(git_dir, algorithm)?;
    let logs = read_reftable_log_stack(git_dir, algorithm)?;
    write_reftable_stack_with_logs_locked(git_dir, algorithm, &refs, &logs, lock)
}

struct ReftableStackLock {
    target_path: PathBuf,
    lock_path: PathBuf,
    file: Option<fs::File>,
    committed: bool,
}

#[derive(Clone, Copy)]
enum ReftableLockTimeout {
    Infinite,
    Millis(u64),
}

impl ReftableStackLock {
    fn commit(mut self, bytes: &[u8]) -> io::Result<()> {
        let mut file = self
            .file
            .take()
            .ok_or_else(|| io::Error::other("reftable stack lock is already closed"))?;
        file.set_len(0)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        replace_with_lock(&self.lock_path, &self.target_path)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for ReftableStackLock {
    fn drop(&mut self) {
        if !self.committed {
            self.file.take();
            let _ = fs::remove_file(&self.lock_path);
        }
    }
}

fn acquire_reftable_stack_lock(reftable_dir: &Path) -> io::Result<ReftableStackLock> {
    let target_path = reftable_dir.join("tables.list");
    let lock_path = lock_path(&target_path);
    let timeout = reftable_lock_timeout(reftable_dir)?;
    let started = Instant::now();
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => {
                return Ok(ReftableStackLock {
                    target_path,
                    lock_path,
                    file: Some(file),
                    committed: false,
                });
            }
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists
                    && reftable_lock_should_retry(timeout, started.elapsed()) =>
            {
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("Unable to create '{}': {error}", lock_path.display()),
                ));
            }
            Err(error) => return Err(error),
        }
    }
}

fn reftable_lock_should_retry(timeout: ReftableLockTimeout, elapsed: Duration) -> bool {
    match timeout {
        ReftableLockTimeout::Infinite => true,
        ReftableLockTimeout::Millis(milliseconds) => elapsed < Duration::from_millis(milliseconds),
    }
}

fn reftable_lock_timeout(reftable_dir: &Path) -> io::Result<ReftableLockTimeout> {
    let configured = std::env::var(REFTABLE_LOCK_TIMEOUT_ENV)
        .ok()
        .or_else(reftable_lock_timeout_from_environment)
        .or_else(|| reftable_lock_timeout_from_config(reftable_dir));
    parse_reftable_lock_timeout(configured.as_deref().unwrap_or("100"))
}

fn reftable_write_options(reftable_dir: &Path) -> io::Result<ReftableWriteOptions> {
    let block_size = reftable_write_option_value(
        reftable_dir,
        REFTABLE_BLOCK_SIZE_ENV,
        "reftable.blocksize",
        "blocksize",
    )
    .map(|value| parse_reftable_unsigned_size(&value, "reftable.blocksize"))
    .transpose()?
    .unwrap_or(4096);
    if block_size >= 1 << 24 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reftable block size cannot exceed 16MB",
        ));
    }
    let restart_interval = reftable_write_option_value(
        reftable_dir,
        REFTABLE_RESTART_INTERVAL_ENV,
        "reftable.restartinterval",
        "restartinterval",
    )
    .map(|value| parse_reftable_unsigned_size(&value, "reftable.restartinterval"))
    .transpose()?
    .unwrap_or(16);
    if restart_interval > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reftable block size cannot exceed 65535",
        ));
    }
    let index_objects = reftable_write_option_value(
        reftable_dir,
        REFTABLE_INDEX_OBJECTS_ENV,
        "reftable.indexobjects",
        "indexobjects",
    )
    .map(|value| parse_reftable_bool(&value, "reftable.indexobjects"))
    .transpose()?
    .unwrap_or(true);
    Ok(ReftableWriteOptions {
        block_size: if block_size == 0 { 4096 } else { block_size },
        restart_interval: if restart_interval == 0 {
            16
        } else {
            restart_interval
        },
        index_objects,
    })
}

fn reftable_write_option_value(
    reftable_dir: &Path,
    override_env: &str,
    command_key: &str,
    config_key: &str,
) -> Option<String> {
    std::env::var(override_env)
        .ok()
        .or_else(|| reftable_config_value_from_environment(command_key))
        .or_else(|| reftable_config_value_from_file(reftable_dir, config_key))
}

fn reftable_config_value_from_environment(name: &str) -> Option<String> {
    let count = std::env::var("GIT_CONFIG_COUNT")
        .ok()?
        .parse::<usize>()
        .ok()?;
    let mut value = None;
    for index in 0..count {
        let key = std::env::var(format!("GIT_CONFIG_KEY_{index}")).ok()?;
        if key.eq_ignore_ascii_case(name) {
            value = std::env::var(format!("GIT_CONFIG_VALUE_{index}")).ok();
        }
    }
    value
}

fn reftable_config_value_from_file(reftable_dir: &Path, wanted_key: &str) -> Option<String> {
    let git_dir = reftable_dir.parent()?;
    let raw = fs::read_to_string(git_dir.join("config")).ok()?;
    let mut in_reftable = false;
    let mut value = None;
    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            let section = line[1..line.len() - 1].trim();
            in_reftable = section.eq_ignore_ascii_case("reftable");
            continue;
        }
        if !in_reftable || line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case(wanted_key) {
            value = Some(raw_value.trim().to_owned());
        }
    }
    value
}

fn parse_reftable_unsigned_size(raw: &str, name: &str) -> io::Result<usize> {
    let (number, multiplier) = match raw.as_bytes().last().copied() {
        Some(b'k' | b'K') => (&raw[..raw.len() - 1], 1024_usize),
        Some(b'm' | b'M') => (&raw[..raw.len() - 1], 1024_usize.pow(2)),
        Some(b'g' | b'G') => (&raw[..raw.len() - 1], 1024_usize.pow(3)),
        _ => (raw, 1_usize),
    };
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad numeric config value '{raw}' for '{name}': invalid unit"),
        )
    };
    number
        .parse::<usize>()
        .ok()
        .and_then(|value| value.checked_mul(multiplier))
        .ok_or_else(invalid)
}

fn parse_reftable_bool(raw: &str, name: &str) -> io::Result<bool> {
    match raw.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" | "" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad boolean config value '{raw}' for '{name}'"),
        )),
    }
}

fn reftable_lock_timeout_from_environment() -> Option<String> {
    let count = std::env::var("GIT_CONFIG_COUNT")
        .ok()?
        .parse::<usize>()
        .ok()?;
    let mut value = None;
    for index in 0..count {
        let key = std::env::var(format!("GIT_CONFIG_KEY_{index}")).ok()?;
        if key.eq_ignore_ascii_case("reftable.locktimeout") {
            value = std::env::var(format!("GIT_CONFIG_VALUE_{index}")).ok();
        }
    }
    value
}

fn reftable_lock_timeout_from_config(reftable_dir: &Path) -> Option<String> {
    let git_dir = reftable_dir.parent()?;
    let raw = fs::read_to_string(git_dir.join("config")).ok()?;
    let mut in_reftable = false;
    let mut value = None;
    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            let section = line[1..line.len() - 1].trim();
            in_reftable = section.eq_ignore_ascii_case("reftable");
            continue;
        }
        if !in_reftable || line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("locktimeout") {
            value = Some(raw_value.trim().to_owned());
        }
    }
    value
}

fn parse_reftable_lock_timeout(raw: &str) -> io::Result<ReftableLockTimeout> {
    let milliseconds = parse_reftable_lock_timeout_millis(raw)?;
    match milliseconds {
        -1 => Ok(ReftableLockTimeout::Infinite),
        0.. => Ok(ReftableLockTimeout::Millis(milliseconds as u64)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "reftable lock timeout does not support negative values other than -1",
        )),
    }
}

pub fn parse_reftable_lock_timeout_millis(raw: &str) -> io::Result<i64> {
    let (number, multiplier) = match raw.as_bytes().last().copied() {
        Some(b'k' | b'K') => (&raw[..raw.len() - 1], 1024_i64),
        Some(b'm' | b'M') => (&raw[..raw.len() - 1], 1024_i64.pow(2)),
        Some(b'g' | b'G') => (&raw[..raw.len() - 1], 1024_i64.pow(3)),
        _ => (raw, 1_i64),
    };
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad numeric config value '{raw}' for 'reftable.locktimeout': invalid unit"),
        )
    };
    number
        .parse::<i64>()
        .ok()
        .and_then(|value| value.checked_mul(multiplier))
        .ok_or_else(invalid)
}

fn write_reftable_dummy_files(git_dir: &Path) -> io::Result<()> {
    atomic_write(git_dir.join("HEAD"), b"ref: refs/heads/.invalid\n")?;
    let refs_dir = git_dir.join("refs");
    fs::create_dir_all(&refs_dir)?;
    let heads = refs_dir.join("heads");
    if heads.is_dir() {
        fs::remove_dir_all(&heads)?;
    }
    atomic_write(heads, b"this repository uses the reftable format\n")?;
    let tags = refs_dir.join("tags");
    if tags.is_dir() {
        fs::remove_dir_all(tags)?;
    }
    Ok(())
}

fn next_reftable_update_index(reftable_dir: &Path) -> io::Result<u64> {
    let tables = match fs::read_to_string(reftable_dir.join("tables.list")) {
        Ok(tables) => tables,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(1),
        Err(error) => return Err(error),
    };
    let Some(last_table) = tables
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
    else {
        return Ok(1);
    };
    let bytes = fs::read(reftable_dir.join(last_table))?;
    let algorithm = if bytes.get(4).copied() == Some(2) && bytes.get(24..28) == Some(b"s256") {
        GitHashAlgorithm::Sha256
    } else {
        GitHashAlgorithm::Sha1
    };
    parse_reftable_header(&bytes, algorithm)?
        .max_update_index
        .checked_add(1)
        .ok_or_else(|| reftable_invalid("reftable update index overflow"))
}

fn reftable_table_name(min_update_index: u64, max_update_index: u64) -> String {
    let mut name = String::with_capacity(REFTABLE_TABLE_NAME_CAPACITY);
    name.push_str(&format!(
        "0x{min_update_index:012x}-0x{max_update_index:012x}-zmin.ref"
    ));
    name
}

fn encode_reftable(
    min_update_index: u64,
    max_update_index: u64,
    refs: &BTreeMap<String, Option<RefTarget>>,
    logs: &[ParsedReftableLogRecord],
    algorithm: GitHashAlgorithm,
    options: ReftableWriteOptions,
) -> io::Result<Vec<u8>> {
    let refs = refs
        .iter()
        .map(|(name, target)| {
            validate_storable_ref_name(name)?;
            let mut value = Vec::new();
            write_reftable_varint(&mut value, 0);
            let (value_type, object_ids) = match target {
                None => (0, Vec::new()),
                Some(RefTarget::Direct(id)) => {
                    if id.algorithm() != algorithm {
                        return Err(reftable_invalid(
                            "object id algorithm does not match reftable writer",
                        ));
                    }
                    value.extend_from_slice(id.as_bytes());
                    (1, vec![id.as_bytes().to_vec()])
                }
                Some(RefTarget::Symbolic(target)) => {
                    if target != "refs/heads/.invalid" {
                        validate_storable_ref_name(target)?;
                    }
                    write_reftable_varint(&mut value, target.len() as u64);
                    value.extend_from_slice(target.as_bytes());
                    (3, Vec::new())
                }
            };
            Ok(ReftableEncodedRecord {
                key: name.as_bytes().to_vec(),
                value_type,
                value,
                object_ids,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    let mut logs = logs
        .iter()
        .map(|record| {
            let (ref_name, update_index) = record.key();
            validate_storable_ref_name(&ref_name)?;
            if let ParsedReftableLogRecord::Update(record) = record
                && (record.old_id.algorithm() != algorithm
                    || record.new_id.algorithm() != algorithm)
            {
                return Err(reftable_invalid(
                    "object id algorithm does not match reftable log writer",
                ));
            }
            let mut key = ref_name.into_bytes();
            key.push(0);
            key.extend_from_slice(&(u64::MAX - update_index).to_be_bytes());
            let mut value = Vec::new();
            let value_type = match record {
                ParsedReftableLogRecord::Deletion { .. } => 0,
                ParsedReftableLogRecord::Update(record) => {
                    value.extend_from_slice(record.old_id.as_bytes());
                    value.extend_from_slice(record.new_id.as_bytes());
                    write_reftable_encoded_string(&mut value, &record.name);
                    write_reftable_encoded_string(&mut value, &record.email);
                    write_reftable_varint(&mut value, record.timestamp);
                    value.extend_from_slice(&record.timezone_offset.to_be_bytes());
                    let message = if record.message.ends_with('\n') {
                        record.message.clone()
                    } else {
                        format!("{}\n", record.message)
                    };
                    write_reftable_encoded_string(&mut value, &message);
                    1
                }
            };
            Ok(ReftableEncodedRecord {
                key,
                value_type,
                value,
                object_ids: Vec::new(),
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    logs.sort_by(|left, right| left.key.cmp(&right.key));
    encode_reftable_table(
        algorithm,
        min_update_index,
        max_update_index,
        &refs,
        &logs,
        options,
    )
}

fn write_reftable_encoded_string(out: &mut Vec<u8>, value: &str) {
    write_reftable_varint(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn read_reftable_file(
    path: &Path,
    algorithm: GitHashAlgorithm,
) -> io::Result<Vec<(String, Option<RefTarget>)>> {
    let bytes = fs::read(path)?;
    let header = parse_reftable_header(&bytes, algorithm)?;
    let mut refs = Vec::new();
    let mut block_start = header.header_len;
    while block_start + REFTABLE_BLOCK_HEADER_LEN <= bytes.len() {
        if &bytes[block_start..block_start + 4] == b"REFT" {
            break;
        }
        let block_type = bytes[block_start];
        if block_type != b'r' {
            break;
        }
        if block_type == 0 {
            break;
        }
        let block_len = read_u24(&bytes, block_start + 1)?;
        if block_len == 0 {
            break;
        }
        let block_end = if block_start == header.header_len {
            block_len
        } else {
            block_start
                .checked_add(block_len)
                .ok_or_else(|| reftable_invalid("reftable block length overflow"))?
        };
        if block_end > bytes.len() || block_end <= block_start + REFTABLE_BLOCK_HEADER_LEN {
            return Err(reftable_invalid("invalid reftable block length"));
        }
        parse_reftable_ref_block(
            &bytes,
            block_start,
            block_end,
            header.min_update_index,
            algorithm,
            &mut refs,
        )?;
        block_start = if header.block_size != 0 {
            let next = if block_start == header.header_len {
                header.block_size
            } else {
                block_start
                    .checked_add(header.block_size)
                    .ok_or_else(|| reftable_invalid("reftable block offset overflow"))?
            };
            if next <= block_start { block_end } else { next }
        } else {
            block_end
        };
    }
    Ok(refs)
}

#[derive(Debug, Clone, Copy)]
struct ReftableHeader {
    header_len: usize,
    block_size: usize,
    min_update_index: u64,
    max_update_index: u64,
}

fn parse_reftable_header(bytes: &[u8], algorithm: GitHashAlgorithm) -> io::Result<ReftableHeader> {
    if bytes.len() < REFTABLE_HEADER_V1_LEN || &bytes[0..4] != b"REFT" {
        return Err(reftable_invalid("invalid reftable header"));
    }
    let version = bytes[4];
    let block_size = read_u24(bytes, 5)?;
    let min_update_index = read_u64(bytes, 8)?;
    let max_update_index = read_u64(bytes, 16)?;
    match version {
        1 => {
            if algorithm != GitHashAlgorithm::Sha1 {
                return Err(reftable_invalid("reftable v1 requires sha1"));
            }
            Ok(ReftableHeader {
                header_len: REFTABLE_HEADER_V1_LEN,
                block_size,
                min_update_index,
                max_update_index,
            })
        }
        2 => {
            if bytes.len() < REFTABLE_HEADER_V2_LEN {
                return Err(reftable_invalid("truncated reftable v2 header"));
            }
            let hash_id = &bytes[24..28];
            let expected = match algorithm {
                GitHashAlgorithm::Sha1 => b"sha1".as_slice(),
                GitHashAlgorithm::Sha256 => b"s256".as_slice(),
            };
            if hash_id != expected {
                return Err(reftable_invalid("reftable hash id mismatch"));
            }
            Ok(ReftableHeader {
                header_len: REFTABLE_HEADER_V2_LEN,
                block_size,
                min_update_index,
                max_update_index,
            })
        }
        _ => Err(reftable_invalid("unsupported reftable version")),
    }
}

fn parse_reftable_ref_block(
    bytes: &[u8],
    block_start: usize,
    block_end: usize,
    min_update_index: u64,
    algorithm: GitHashAlgorithm,
    refs: &mut Vec<(String, Option<RefTarget>)>,
) -> io::Result<()> {
    let restart_count_offset = block_end
        .checked_sub(REFTABLE_RESTART_COUNT_LEN)
        .ok_or_else(|| reftable_invalid("truncated reftable restart count"))?;
    let restart_count =
        u16::from_be_bytes([bytes[restart_count_offset], bytes[restart_count_offset + 1]]) as usize;
    let record_end = restart_count_offset
        .checked_sub(
            restart_count
                .checked_mul(3)
                .ok_or_else(|| reftable_invalid("reftable restart table length overflow"))?,
        )
        .ok_or_else(|| reftable_invalid("truncated reftable restart table"))?;
    let mut cursor = block_start + REFTABLE_BLOCK_HEADER_LEN;
    let mut prior_name = Vec::new();
    while cursor < record_end {
        let prefix_len = read_reftable_varint(bytes, &mut cursor)?;
        let suffix_and_type = read_reftable_varint(bytes, &mut cursor)?;
        let suffix_len = (suffix_and_type >> 3) as usize;
        let value_type = (suffix_and_type & 0x7) as u8;
        if prefix_len as usize > prior_name.len() || cursor + suffix_len > record_end {
            return Err(reftable_invalid("invalid reftable ref name"));
        }
        let mut name = prior_name[..prefix_len as usize].to_vec();
        name.extend_from_slice(&bytes[cursor..cursor + suffix_len]);
        cursor += suffix_len;
        let _update_index = min_update_index
            .checked_add(read_reftable_varint(bytes, &mut cursor)?)
            .ok_or_else(|| reftable_invalid("reftable update index overflow"))?;
        let name = String::from_utf8(name).map_err(|_| reftable_invalid("non-utf8 ref name"))?;
        let target = match value_type {
            0 => None,
            1 | 2 => {
                let id = read_reftable_object_id(bytes, &mut cursor, algorithm, record_end)?;
                if value_type == 2 {
                    let _peeled =
                        read_reftable_object_id(bytes, &mut cursor, algorithm, record_end)?;
                }
                Some(RefTarget::Direct(id))
            }
            3 => {
                let target_len = read_reftable_varint(bytes, &mut cursor)? as usize;
                if cursor + target_len > record_end {
                    return Err(reftable_invalid("truncated symbolic reftable ref"));
                }
                let target = String::from_utf8(bytes[cursor..cursor + target_len].to_vec())
                    .map_err(|_| reftable_invalid("non-utf8 symbolic reftable ref"))?;
                cursor += target_len;
                Some(RefTarget::Symbolic(target))
            }
            _ => return Err(reftable_invalid("unsupported reftable ref value type")),
        };
        prior_name = name.as_bytes().to_vec();
        if validate_storable_ref_name(&name).is_ok() {
            refs.push((name, target));
        }
    }
    Ok(())
}

fn read_reftable_object_id(
    bytes: &[u8],
    cursor: &mut usize,
    algorithm: GitHashAlgorithm,
    record_end: usize,
) -> io::Result<ObjectId> {
    let len = match algorithm {
        GitHashAlgorithm::Sha1 => 20,
        GitHashAlgorithm::Sha256 => 32,
    };
    if *cursor + len > record_end {
        return Err(reftable_invalid("truncated reftable object id"));
    }
    let id = ObjectId::new(algorithm, &bytes[*cursor..*cursor + len]);
    *cursor += len;
    Ok(id)
}

fn read_reftable_varint(bytes: &[u8], cursor: &mut usize) -> io::Result<u64> {
    if *cursor >= bytes.len() {
        return Err(reftable_invalid("truncated reftable varint"));
    }
    let mut value = (bytes[*cursor] & 0x7f) as u64;
    while bytes[*cursor] & 0x80 != 0 {
        *cursor += 1;
        if *cursor >= bytes.len() {
            return Err(reftable_invalid("truncated reftable varint"));
        }
        value = value
            .checked_add(1)
            .and_then(|value| value.checked_shl(7))
            .map(|value| value | (bytes[*cursor] & 0x7f) as u64)
            .ok_or_else(|| reftable_invalid("reftable varint overflow"))?;
    }
    *cursor += 1;
    Ok(value)
}

fn write_reftable_varint(out: &mut Vec<u8>, value: u64) {
    let mut parts = vec![(value & 0x7f) as u8];
    let mut value = value >> 7;
    while value != 0 {
        value -= 1;
        parts.push(((value & 0x7f) as u8) | 0x80);
        value >>= 7;
    }
    out.extend(parts.into_iter().rev());
}

fn read_u24(bytes: &[u8], offset: usize) -> io::Result<usize> {
    if offset + 3 > bytes.len() {
        return Err(reftable_invalid("truncated uint24"));
    }
    Ok(((bytes[offset] as usize) << 16)
        | ((bytes[offset + 1] as usize) << 8)
        | bytes[offset + 2] as usize)
}

fn read_u64(bytes: &[u8], offset: usize) -> io::Result<u64> {
    if offset + 8 > bytes.len() {
        return Err(reftable_invalid("truncated uint64"));
    }
    Ok(u64::from_be_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("slice length checked"),
    ))
}

fn reftable_invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn check_ref_format(name: &str, allow_onelevel: bool) -> bool {
    validate_ref_format(name, allow_onelevel).is_ok()
}

fn prune_empty_ref_parent_dirs(git_dir: &Path, ref_path: &Path) -> io::Result<()> {
    let refs_root = git_dir.join("refs");
    let mut dir = ref_path.parent();
    while let Some(path) = dir {
        if path == refs_root || path.parent() == Some(refs_root.as_path()) {
            break;
        }
        match fs::remove_dir(path) {
            Ok(()) => dir = path.parent(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => dir = path.parent(),
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn validate_ref_name(name: &str) -> io::Result<()> {
    if !name.starts_with("refs/") || validate_ref_format(name, false).is_err() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid git ref name",
        ));
    }
    Ok(())
}

fn validate_storable_ref_name(name: &str) -> io::Result<()> {
    if is_valid_pseudoref_name(name) || validate_ref_format(name, true).is_ok() {
        return Ok(());
    }
    validate_ref_name(name)
}

fn validate_ref_lookup_name(name: &str) -> io::Result<()> {
    if validate_storable_ref_name(name).is_ok() {
        return Ok(());
    }
    if name.starts_with("refs/")
        && !name
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid git ref name",
    ))
}

fn validate_ref_format(name: &str, allow_onelevel: bool) -> io::Result<()> {
    if name.is_empty()
        || name == "@"
        || name.ends_with('/')
        || name.ends_with('.')
        || name.contains("//")
        || (!allow_onelevel && !name.contains('/'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid git ref name",
        ));
    }
    if name.split('/').any(|part| {
        part.is_empty()
            || part == "."
            || part == ".."
            || part.starts_with('.')
            || part.ends_with(".lock")
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid git ref component",
        ));
    }
    if name.contains("..")
        || name.contains('\\')
        || name.contains("@{")
        || name.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
        || name
            .bytes()
            .any(|byte| matches!(byte, b' ' | b'~' | b'^' | b':' | b'?' | b'*' | b'['))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid git ref character",
        ));
    }
    Ok(())
}

fn validate_ref_prefix(prefix: &str) -> io::Result<()> {
    if !prefix.starts_with("refs/") || prefix.contains("//") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid git ref prefix",
        ));
    }
    for part in prefix.trim_end_matches('/').split('/') {
        if part.is_empty() || part == "." || part == ".." || part.ends_with(".lock") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid git ref prefix component",
            ));
        }
    }
    if prefix.contains("..")
        || prefix.contains('\\')
        || prefix.contains("@{")
        || prefix
            .bytes()
            .any(|byte| matches!(byte, b' ' | b'~' | b'^' | b':' | b'?' | b'*' | b'['))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid git ref prefix character",
        ));
    }
    Ok(())
}

fn is_valid_pseudoref_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.ends_with(".lock")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn is_migratable_root_ref_name(name: &str) -> bool {
    name != "HEAD" && name != "FETCH_HEAD" && name != "MERGE_HEAD" && is_valid_pseudoref_name(name)
}

fn inspect_ref_path(
    path: &Path,
    algorithm: GitHashAlgorithm,
    allow_onelevel_symbolic_target: bool,
) -> io::Result<RefPathState> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(RefPathState::Missing),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        return match parse_symbolic_link_ref_target(path, allow_onelevel_symbolic_target) {
            Ok(target) => Ok(RefPathState::Valid(target)),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData
                ) =>
            {
                Ok(RefPathState::BrokenFile)
            }
            Err(error) => Err(error),
        };
    }
    if metadata.file_type().is_dir() {
        return if dir_contains_only_empty_dirs(path)? {
            Ok(RefPathState::EmptyDir)
        } else {
            Ok(RefPathState::BlockingDir)
        };
    }
    let raw = fs::read_to_string(path)?;
    match parse_ref_target(
        algorithm,
        raw.trim_end_matches('\n'),
        allow_onelevel_symbolic_target,
    ) {
        Ok(target) => Ok(RefPathState::Valid(target)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData
            ) =>
        {
            Ok(RefPathState::BrokenFile)
        }
        Err(error) => Err(error),
    }
}

fn prepare_ref_path_for_write(
    path: &Path,
    algorithm: GitHashAlgorithm,
    allow_onelevel_symbolic_target: bool,
) -> io::Result<()> {
    match inspect_ref_path(path, algorithm, allow_onelevel_symbolic_target)? {
        RefPathState::Missing | RefPathState::Valid(_) => Ok(()),
        RefPathState::BrokenFile => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "reference broken",
        )),
        RefPathState::EmptyDir => fs::remove_dir_all(path),
        RefPathState::BlockingDir => Err(io::Error::new(
            io::ErrorKind::IsADirectory,
            "non-empty ref directory",
        )),
    }
}

fn dir_contains_only_empty_dirs(path: &Path) -> io::Result<bool> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if !dir_contains_only_empty_dirs(&entry.path())? {
                return Ok(false);
            }
            continue;
        }
        return Ok(false);
    }
    Ok(true)
}

fn collect_loose_refs(git_dir: &Path, prefix: &str, refs: &mut BTreeSet<String>) -> io::Result<()> {
    let root = git_dir.join(prefix);
    if !root.exists() {
        return Ok(());
    }
    collect_loose_refs_from_dir(&root, prefix.trim_end_matches('/'), refs)
}

fn should_skip_resolved_ref_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound
            | io::ErrorKind::NotADirectory
            | io::ErrorKind::IsADirectory
            | io::ErrorKind::InvalidData
    )
}

fn collect_loose_refs_from_dir(
    dir: &Path,
    prefix: &str,
    refs: &mut BTreeSet<String>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let ref_name = format!("{prefix}/{name}");
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_loose_refs_from_dir(&entry.path(), &ref_name, refs)?;
        } else if file_type.is_file() {
            if name.ends_with(".lock") {
                continue;
            }
            refs.insert(ref_name);
        }
    }
    Ok(())
}

fn for_each_packed_ref_line<F>(git_dir: &Path, mut on_line: F) -> io::Result<()>
where
    F: FnMut(&str) -> io::Result<bool>,
{
    let file = match fs::File::open(git_dir.join("packed-refs")) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    let mut reader = packed_refs_reader(file);
    let mut line = packed_ref_line_buffer();
    while reader.read_line(&mut line)? != 0 {
        let terminated = line.ends_with('\n');
        if terminated {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        if !terminated {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unterminated line in .git/packed-refs: {line}"),
            ));
        }
        if !on_line(&line)? {
            return Ok(());
        }
        line.clear();
    }
    Ok(())
}

fn delete_packed_ref(algorithm: GitHashAlgorithm, git_dir: &Path, name: &str) -> io::Result<bool> {
    let path = git_dir.join("packed-refs");
    let scan_file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    if !packed_refs_contains_ref(algorithm, scan_file, name)? {
        return Ok(false);
    }
    let packed_lock = acquire_packed_refs_lock(git_dir)?;
    let file = fs::File::open(&path)?;
    let temp_path = packed_refs_temp_path(git_dir);
    match write_packed_refs_without_ref(algorithm, file, &temp_path, name) {
        Ok(()) => {
            fs::rename(&temp_path, &path)?;
            drop(packed_lock);
            let _ = fs::remove_file(packed_refs_lock_path(git_dir));
            Ok(true)
        }
        Err(error) => {
            drop(packed_lock);
            let _ = fs::remove_file(packed_refs_lock_path(git_dir));
            let _ = fs::remove_file(&temp_path);
            Err(error)
        }
    }
}

fn packed_refs_lock_path(git_dir: &Path) -> PathBuf {
    git_dir.join("packed-refs.lock")
}

fn packed_refs_temp_path(git_dir: &Path) -> PathBuf {
    git_dir.join("packed-refs.new")
}

fn acquire_packed_refs_lock(git_dir: &Path) -> io::Result<fs::File> {
    let lock_path = packed_refs_lock_path(git_dir);
    let timeout = packed_refs_timeout();
    let start = Instant::now();
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => return Ok(file),
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists && start.elapsed() < timeout =>
            {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("Unable to create '{}': {error}", lock_path.display()),
                ));
            }
            Err(error) => return Err(error),
        }
    }
}

fn packed_refs_timeout() -> Duration {
    Duration::from_millis(read_packed_refs_timeout_ms().unwrap_or(2000))
}

fn read_packed_refs_timeout_ms() -> Option<u64> {
    read_packed_refs_timeout_ms_from_count().or_else(read_packed_refs_timeout_ms_from_parameters)
}

fn read_packed_refs_timeout_ms_from_count() -> Option<u64> {
    let raw_count = std::env::var("GIT_CONFIG_COUNT").ok()?;
    let count = raw_count.parse::<usize>().ok()?;
    for index in 0..count {
        let key = std::env::var(format!("GIT_CONFIG_KEY_{index}")).ok()?;
        if !key.eq_ignore_ascii_case("core.packedrefstimeout") {
            continue;
        }
        let value = std::env::var(format!("GIT_CONFIG_VALUE_{index}")).ok()?;
        if let Ok(parsed) = value.parse::<u64>() {
            return Some(parsed);
        }
    }
    None
}

fn read_packed_refs_timeout_ms_from_parameters() -> Option<u64> {
    let raw = std::env::var("GIT_CONFIG_PARAMETERS").ok()?;
    let cleaned = raw.replace('\'', "");
    for word in cleaned.split_whitespace() {
        let normalized = word.trim();
        let (key, value) = normalized.split_once('=')?;
        if key.eq_ignore_ascii_case("core.packedrefstimeout")
            && let Ok(parsed) = value.parse::<u64>()
        {
            return Some(parsed);
        }
    }
    None
}

fn packed_refs_contains_ref(
    algorithm: GitHashAlgorithm,
    file: fs::File,
    name: &str,
) -> io::Result<bool> {
    let mut reader = packed_refs_reader(file);
    let mut line = packed_ref_line_buffer();
    while reader.read_line(&mut line)? != 0 {
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        if let Some((_, ref_name)) = parse_packed_ref_line(algorithm, &line)?
            && ref_name == name
        {
            return Ok(true);
        }
        line.clear();
    }
    Ok(false)
}

fn write_packed_refs_without_ref(
    algorithm: GitHashAlgorithm,
    file: fs::File,
    lock_path: &Path,
    name: &str,
) -> io::Result<()> {
    let mut skip_peeled = false;
    let mut reader = packed_refs_reader(file);
    let mut lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path)?;
    {
        let mut writer = packed_refs_writer(&mut lock);
        let mut line = packed_ref_line_buffer();
        while reader.read_line(&mut line)? != 0 {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            if skip_peeled && line.starts_with('^') {
                skip_peeled = false;
                line.clear();
                continue;
            }
            skip_peeled = false;
            if let Some((_, ref_name)) = parse_packed_ref_line(algorithm, &line)?
                && ref_name == name
            {
                skip_peeled = true;
                line.clear();
                continue;
            }
            if line.starts_with("# pack-refs with:") {
                line = canonical_packed_refs_header(&line);
            }
            writer.write_all(line.as_bytes())?;
            writer.write_all(b"\n")?;
            line.clear();
        }
        writer.flush()?;
    }
    lock.sync_all()?;
    Ok(())
}

fn canonical_packed_refs_header(line: &str) -> String {
    let capabilities = line
        .strip_prefix("# pack-refs with:")
        .unwrap_or_default()
        .split_whitespace()
        .filter(|capability| matches!(*capability, "peeled" | "fully-peeled" | "sorted"))
        .collect::<Vec<_>>();
    if capabilities.is_empty() {
        "# pack-refs with:".to_owned()
    } else {
        format!("# pack-refs with: {} ", capabilities.join(" "))
    }
}

fn packed_refs_reader(file: fs::File) -> io::BufReader<fs::File> {
    io::BufReader::with_capacity(PACKED_REFS_IO_BUFFER_CAPACITY, file)
}

fn packed_refs_writer(file: &mut fs::File) -> io::BufWriter<&mut fs::File> {
    io::BufWriter::with_capacity(PACKED_REFS_IO_BUFFER_CAPACITY, file)
}

fn packed_ref_line_buffer() -> String {
    String::with_capacity(PACKED_REF_LINE_INITIAL_CAPACITY)
}

fn parse_packed_ref_line(
    algorithm: GitHashAlgorithm,
    line: &str,
) -> io::Result<Option<(ObjectId, String)>> {
    if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
        return Ok(None);
    }
    let mut parts = line.split(' ');
    let id = match parts.next() {
        Some(id) => id,
        None => return Ok(None),
    };
    let name = parts
        .next()
        .ok_or_else(|| packed_refs_unexpected_line(line))?;
    if parts.next().is_some() {
        return Err(packed_refs_unexpected_line(line));
    }
    validate_ref_name(name).map_err(|_| packed_refs_unexpected_line(line))?;
    ObjectId::from_hex(algorithm, id)
        .map(|id| Some((id, name.to_owned())))
        .map_err(|_| packed_refs_unexpected_line(line))
}

fn packed_refs_unexpected_line(line: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("unexpected line in .git/packed-refs: {line}"),
    )
}

fn write_packed_refs_file(git_dir: &Path, bytes: &[u8]) -> io::Result<()> {
    let path = git_dir.join("packed-refs");
    let final_path = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let target = fs::read_link(&path)?;
            if target.is_absolute() {
                target
            } else {
                git_dir.join(target)
            }
        }
        Ok(_) | Err(_) => path.clone(),
    };
    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock = acquire_packed_refs_lock(git_dir)?;
    let lock_path = packed_refs_lock_path(git_dir);
    let result = (|| {
        fs::write(&lock_path, bytes)?;
        replace_with_lock(&lock_path, &final_path)
    })();
    drop(lock);
    if result.is_err() {
        let _ = fs::remove_file(&lock_path);
    }
    result
}

fn atomic_write(path: PathBuf, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock_path = lock_path(&path);
    let write_result = write_lock_file(&lock_path, bytes);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&lock_path);
        return Err(error);
    }
    if let Err(error) = replace_with_lock(&lock_path, &path) {
        let _ = fs::remove_file(&lock_path);
        return Err(error);
    }
    Ok(())
}

fn write_lock_file(lock_path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path)?;
    lock.write_all(bytes)
}

fn resolve_ref_storage_location(git_dir: &Path, include_environment: bool) -> RefStorageLocation {
    if include_environment
        && let Some(value) = std::env::var_os("GIT_REFERENCE_BACKEND")
        && !value.is_empty()
    {
        return parse_ref_storage_location(git_dir, &value.to_string_lossy());
    }
    let raw = match fs::read_to_string(git_dir.join("config")) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return default_ref_storage_location(git_dir);
        }
        Err(error) => return RefStorageLocation::Invalid(error.to_string()),
    };
    let mut section = String::new();
    let mut configured = None;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = config_section_name(name);
            continue;
        }
        if section.eq_ignore_ascii_case("extensions")
            && let Some((name, value)) = line.split_once('=')
            && name.trim().eq_ignore_ascii_case("refStorage")
        {
            configured = Some(value.trim().to_owned());
        }
    }
    configured
        .as_deref()
        .map(|value| parse_ref_storage_location(git_dir, value))
        .unwrap_or_else(|| default_ref_storage_location(git_dir))
}

fn default_ref_storage_location(git_dir: &Path) -> RefStorageLocation {
    if git_dir.join("reftable/tables.list").is_file() && git_dir.join("refs/heads").is_file() {
        return RefStorageLocation::Valid {
            kind: RefStorageKind::Reftable,
            root: git_dir.to_path_buf(),
        };
    }
    RefStorageLocation::Valid {
        kind: RefStorageKind::Files,
        root: git_dir.to_path_buf(),
    }
}

fn parse_ref_storage_location(git_dir: &Path, value: &str) -> RefStorageLocation {
    let (kind, path) = match value {
        "files" | "" => {
            return RefStorageLocation::Valid {
                kind: RefStorageKind::Files,
                root: git_dir.to_path_buf(),
            };
        }
        "reftable" => {
            return RefStorageLocation::Valid {
                kind: RefStorageKind::Reftable,
                root: git_dir.to_path_buf(),
            };
        }
        value => match value.split_once("://") {
            Some(("files", path)) if !path.is_empty() => (RefStorageKind::Files, path),
            Some(("reftable", path)) if !path.is_empty() => (RefStorageKind::Reftable, path),
            _ => return RefStorageLocation::Invalid(value.to_owned()),
        },
    };
    let root = PathBuf::from(path);
    let root = if root.is_absolute() {
        root
    } else {
        git_dir.join(root)
    };
    RefStorageLocation::Valid { kind, root }
}

fn config_section_name(raw: &str) -> String {
    raw.split_whitespace().next().unwrap_or(raw).to_owned()
}

fn lock_path(path: &Path) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(".lock");
    PathBuf::from(value)
}

#[cfg(unix)]
fn replace_with_lock(lock_path: &Path, path: &Path) -> io::Result<()> {
    fs::rename(lock_path, path)
}

#[cfg(windows)]
fn replace_with_lock(lock_path: &Path, path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let mut from = lock_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut to = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            from.as_mut_ptr(),
            to.as_mut_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn replace_with_lock(lock_path: &Path, path: &Path) -> io::Result<()> {
    fs::rename(lock_path, path)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::stock_git_support;
    use crate::{
        GitObjectKind, GitObjectSink, InMemoryObjectStore, LooseObjectStore, Signature, TagBuilder,
    };

    #[test]
    fn writes_ref_readable_by_stock_git() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let id = store
            .write_object(GitObjectKind::Blob, b"ref target\n")
            .expect("write object");
        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        refs.write_ref("refs/heads/main", &id).expect("write ref");
        refs.write_head_symbolic("refs/heads/main")
            .expect("write HEAD");

        assert_eq!(git(&repo, ["rev-parse", "refs/heads/main"]), id.to_hex());
        assert_eq!(git(&repo, ["rev-parse", "HEAD"]), id.to_hex());
    }

    #[test]
    fn storage_kind_defaults_to_files() {
        let repo = git_init();
        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);

        assert_eq!(
            refs.storage_kind().expect("storage kind"),
            RefStorageKind::Files
        );
    }

    #[test]
    fn storage_kind_reads_reftable_config() {
        let repo = git_init();
        std::fs::write(
            repo.path().join(".git/config"),
            "[core]\n\trepositoryformatversion = 1\n[extensions]\n\trefStorage = reftable\n",
        )
        .expect("write config");
        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);

        assert_eq!(
            refs.storage_kind().expect("storage kind"),
            RefStorageKind::Reftable
        );
    }

    #[test]
    fn storage_kind_rejects_unknown_ref_storage() {
        let repo = git_init();
        std::fs::write(
            repo.path().join(".git/config"),
            "[extensions]\n\trefStorage = broken\n",
        )
        .expect("write config");
        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);

        let error = refs.storage_kind().expect_err("invalid storage");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn write_ref_refuses_existing_lock_and_preserves_ref() {
        let repo = git_init();
        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let first = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let second = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        let ref_name = "refs/heads/main";
        refs.write_ref(ref_name, &first).expect("write first ref");
        let ref_path = repo.path().join(".git").join(ref_name);
        let before = std::fs::read(&ref_path).expect("read first ref");
        std::fs::write(lock_path(&ref_path), b"locked").expect("write lock");

        let error = refs
            .write_ref(ref_name, &second)
            .expect_err("write should fail");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(&ref_path).expect("read preserved ref"),
            before
        );
    }

    #[test]
    fn reads_ref_written_by_stock_git() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"ref commit\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "initial"]);
        let expected = git(&repo, ["rev-parse", "HEAD"]);

        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let actual = refs.resolve("HEAD").expect("resolve HEAD");

        assert_eq!(actual.to_hex(), expected);
    }

    #[test]
    fn reads_packed_refs_written_by_stock_git() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"packed ref\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "initial"]);
        git(&repo, ["branch", "feature"]);
        let expected = git(&repo, ["rev-parse", "refs/heads/feature"]);
        git(&repo, ["pack-refs", "--all", "--prune"]);

        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let actual = refs.resolve("refs/heads/feature").expect("resolve feature");
        let names = refs.list_refs("refs/heads/").expect("list refs");

        assert_eq!(actual.to_hex(), expected);
        assert!(names.contains(&"refs/heads/feature".to_owned()));
    }

    #[test]
    fn reads_refs_from_stock_reftable_repo() {
        let repo = git_init_reftable();
        std::fs::write(repo.path().join("README.md"), b"reftable ref\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "initial"]);
        git(&repo, ["branch", "feature"]);
        let expected_head = git(&repo, ["rev-parse", "HEAD"]);
        let expected_feature = git(&repo, ["rev-parse", "refs/heads/feature"]);
        assert_eq!(git(&repo, ["rev-parse", "--show-ref-format"]), "reftable");

        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let actual_head = refs.resolve("HEAD").expect("resolve HEAD");
        let actual_feature = refs.resolve("refs/heads/feature").expect("resolve feature");
        let names = refs.list_refs("refs/heads/").expect("list heads");

        assert_eq!(actual_head.to_hex(), expected_head);
        assert_eq!(actual_feature.to_hex(), expected_feature);
        assert_eq!(
            refs.read_head().expect("read HEAD"),
            RefTarget::Symbolic("refs/heads/main".to_owned())
        );
        assert!(names.contains(&"refs/heads/main".to_owned()));
        assert!(names.contains(&"refs/heads/feature".to_owned()));
    }

    #[test]
    fn reftable_stack_prefers_newest_table() {
        let repo = git_init_reftable();
        std::fs::write(repo.path().join("README.md"), b"base\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "base"]);
        let base = git(&repo, ["rev-parse", "HEAD"]);
        std::fs::write(repo.path().join("README.md"), b"next\n").expect("write file");
        git_env(&repo, ["commit", "-am", "next"]);
        let next = git(&repo, ["rev-parse", "HEAD"]);
        assert_ne!(base, next);

        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let actual = refs.resolve("refs/heads/main").expect("resolve main");

        assert_eq!(actual.to_hex(), next);
    }

    #[test]
    fn reads_all_ref_blocks_from_large_stock_reftable() {
        let repo = git_init_reftable();
        std::fs::write(repo.path().join("README.md"), b"many refs\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "base"]);
        let head = git(&repo, ["rev-parse", "HEAD"]);
        let mut input = String::new();
        for index in 0..1_000 {
            input.push_str(&format!("update refs/heads/bench-{index:04} {head}\n"));
        }
        stock_git_support::git_with_stdin(&repo, &["update-ref", "--stdin"], input.as_bytes());

        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let names = refs
            .list_refs("refs/heads/")
            .expect("read all reftable refs");

        assert_eq!(names.len(), 1_001);
        assert!(names.contains(&"refs/heads/bench-0000".to_owned()));
        assert!(names.contains(&"refs/heads/bench-0999".to_owned()));
        assert!(names.contains(&"refs/heads/main".to_owned()));
    }

    #[test]
    fn writes_reftable_refs_readable_by_stock_git() {
        let repo = git_init();
        std::fs::write(
            repo.path().join(".git/config"),
            "[core]\n\trepositoryformatversion = 1\n[extensions]\n\trefStorage = reftable\n",
        )
        .expect("write config");
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let id = store
            .write_object(GitObjectKind::Blob, b"reftable target\n")
            .expect("write object");
        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);

        refs.write_ref("refs/heads/main", &id).expect("write main");
        refs.write_head_symbolic("refs/heads/main")
            .expect("write HEAD");

        assert_eq!(refs.resolve("HEAD").expect("resolve HEAD"), id);
        assert_eq!(git(&repo, ["rev-parse", "--show-ref-format"]), "reftable");
        assert_eq!(git(&repo, ["rev-parse", "HEAD"]), id.to_hex());
        assert!(!repo.path().join(".git/refs/heads/main").exists());
        assert!(repo.path().join(".git/reftable/tables.list").is_file());
    }

    #[test]
    fn reftable_write_ref_refuses_existing_stack_lock_and_preserves_ref() {
        let repo = git_init();
        let refs = configured_reftable_store(&repo);
        let first = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let second = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        refs.write_ref("refs/heads/main", &first)
            .expect("write initial reftable ref");
        let lock = repo.path().join(".git/reftable/tables.list.lock");
        std::fs::write(&lock, b"locked").expect("write stack lock");

        let error = refs
            .write_ref("refs/heads/main", &second)
            .expect_err("locked stack write should fail");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            refs.resolve("refs/heads/main")
                .expect("read preserved reftable ref"),
            first
        );
        assert!(lock.exists());
    }

    #[test]
    fn reftable_write_ref_honors_configured_lock_timeout() {
        let repo = git_init();
        let refs = configured_reftable_store(&repo);
        let first = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let second = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        refs.write_ref("refs/heads/main", &first)
            .expect("write initial reftable ref");
        let config_path = repo.path().join(".git/config");
        let mut config = std::fs::read_to_string(&config_path).expect("read reftable config");
        config.push_str("[reftable]\n\tlockTimeout = 500\n");
        std::fs::write(&config_path, config).expect("write lock timeout");
        let lock = repo.path().join(".git/reftable/tables.list.lock");
        std::fs::write(&lock, b"locked").expect("write stack lock");
        let lock_to_release = lock.clone();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            std::fs::remove_file(lock_to_release).expect("release stack lock");
        });

        refs.write_ref("refs/heads/main", &second)
            .expect("write after configured lock wait");
        releaser.join().expect("lock releaser panicked");

        assert_eq!(
            refs.resolve("refs/heads/main")
                .expect("read updated reftable ref"),
            second
        );
    }

    #[test]
    fn concurrent_reftable_writers_preserve_every_update() {
        let repo = git_init();
        let refs = configured_reftable_store(&repo);
        let id = ObjectId::new(GitHashAlgorithm::Sha1, &[7; 20]);
        let writers = 6;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(writers));
        let mut handles = Vec::new();
        for index in 0..writers {
            let refs = refs.clone();
            let id = id.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                refs.write_ref(&format!("refs/heads/concurrent-{index}"), &id)
            }));
        }
        for handle in handles {
            handle
                .join()
                .expect("concurrent reftable writer panicked")
                .expect("concurrent reftable writer failed");
        }

        let names = refs
            .list_refs("refs/heads/concurrent-")
            .expect("list concurrent reftable refs");
        assert_eq!(names.len(), writers);
        for index in 0..writers {
            assert!(names.contains(&format!("refs/heads/concurrent-{index}")));
        }
    }

    #[test]
    fn appended_reftable_updates_and_tombstones_are_readable_by_stock_git() {
        let repo = git_init();
        let refs = configured_reftable_store(&repo);
        let first = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let second = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);

        refs.write_ref("refs/heads/main", &first)
            .expect("write initial reftable ref");
        refs.write_ref("refs/heads/main", &second)
            .expect("append reftable update");

        assert_eq!(
            refs.resolve("refs/heads/main").expect("resolve update"),
            second
        );
        assert_eq!(
            git(&repo, ["rev-parse", "refs/heads/main"]),
            second.to_hex()
        );

        refs.delete_ref("refs/heads/main")
            .expect("append reftable tombstone");

        assert!(refs.resolve("refs/heads/main").is_err());
        assert!(
            git_expect_failure(&repo, ["rev-parse", "--verify", "refs/heads/main"])
                .contains("Needed a single revision")
        );
    }

    #[test]
    fn log_only_reftable_delta_preserves_refs_and_reflog() {
        let repo = git_init();
        let refs = configured_reftable_store(&repo);
        let old_id = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let new_id = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        refs.write_ref("refs/heads/main", &new_id)
            .expect("write reftable ref");
        let record = ReftableLogRecord {
            ref_name: "refs/heads/main".to_owned(),
            update_index: 0,
            old_id,
            new_id: new_id.clone(),
            name: "Zmin Test".to_owned(),
            email: "zmin@example.invalid".to_owned(),
            timestamp: 1_700_000_000,
            timezone_offset: 0,
            message: "update\n".to_owned(),
        };

        refs.append_reftable_log(record)
            .expect("append log-only reftable");

        let logs = refs.reftable_logs().expect("read reftable logs");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].update_index, 2);
        assert_eq!(logs[0].new_id, new_id);
        assert_eq!(
            git(&repo, ["rev-parse", "refs/heads/main"]),
            new_id.to_hex()
        );
    }

    #[test]
    fn reftable_compaction_preserves_latest_ref_and_logs() {
        let repo = git_init();
        let refs = configured_reftable_store(&repo);
        let initial = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        refs.write_ref("refs/heads/main", &initial)
            .expect("write initial reftable ref");
        refs.append_reftable_log(ReftableLogRecord {
            ref_name: "refs/heads/main".to_owned(),
            update_index: 0,
            old_id: ObjectId::new(GitHashAlgorithm::Sha1, &[0; 20]),
            new_id: initial,
            name: "Zmin Test".to_owned(),
            email: "zmin@example.invalid".to_owned(),
            timestamp: 1_700_000_000,
            timezone_offset: 0,
            message: "create\n".to_owned(),
        })
        .expect("append reftable log");

        let mut latest = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        for value in 2..REFTABLE_COMPACTION_TABLE_LIMIT {
            latest = ObjectId::new(GitHashAlgorithm::Sha1, &[value as u8; 20]);
            refs.write_ref("refs/heads/main", &latest)
                .expect("append reftable update before compaction");
        }

        assert_eq!(reftable_table_count(&repo), 1);
        assert_eq!(
            refs.resolve("refs/heads/main")
                .expect("resolve compacted ref"),
            latest
        );
        assert_eq!(refs.reftable_logs().expect("read compacted logs").len(), 1);
        assert_eq!(
            git(&repo, ["rev-parse", "refs/heads/main"]),
            latest.to_hex()
        );
    }

    #[test]
    fn fresh_packed_refs_are_readable_by_stock_git_and_refstore() {
        let repo = git_init();
        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let main = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let tag = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        refs.write_fresh_packed_refs(
            &[
                ("refs/remotes/origin/main".to_owned(), main.clone()),
                ("refs/tags/v1".to_owned(), tag.clone()),
            ],
            &[(
                "refs/remotes/origin/HEAD".to_owned(),
                "refs/remotes/origin/main".to_owned(),
            )],
        )
        .expect("write fresh packed refs");

        assert_eq!(
            refs.resolve("refs/remotes/origin/main")
                .expect("resolve packed remote"),
            main
        );
        assert_eq!(
            refs.read_ref("refs/remotes/origin/HEAD")
                .expect("read symbolic remote head"),
            RefTarget::Symbolic("refs/remotes/origin/main".to_owned())
        );
        assert_eq!(
            git(&repo, ["rev-parse", "refs/remotes/origin/main"]),
            main.to_hex()
        );
        assert_eq!(git(&repo, ["rev-parse", "refs/tags/v1"]), tag.to_hex());
        assert_eq!(
            git(&repo, ["symbolic-ref", "refs/remotes/origin/HEAD"]),
            "refs/remotes/origin/main"
        );
    }

    #[test]
    fn ref_name_iterator_uses_loose_ref_over_packed_ref_name_once() {
        let repo = git_init();
        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let packed_id = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let loose_id = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        std::fs::write(
            repo.path().join(".git/packed-refs"),
            format!(
                "{} refs/heads/feature\n{} refs/heads/main\n",
                packed_id.to_hex(),
                packed_id.to_hex()
            ),
        )
        .expect("write packed refs");
        refs.write_ref("refs/heads/feature", &loose_id)
            .expect("write loose ref");

        let mut names = Vec::new();
        refs.for_each_ref_name("refs/heads/", |name| {
            names.push(name.to_owned());
            Ok::<(), io::Error>(())
        })
        .expect("iterate ref names");

        assert_eq!(names, vec!["refs/heads/feature", "refs/heads/main"]);
    }

    #[test]
    fn packed_ref_line_reader_streams_lines_without_materializing_file() {
        let repo = git_init();
        std::fs::write(
            repo.path().join(".git/packed-refs"),
            "# pack-refs with: peeled fully-peeled sorted \r\n\
             1111111111111111111111111111111111111111 refs/heads/one\r\n\
             ^2222222222222222222222222222222222222222\r\n\
             3333333333333333333333333333333333333333 refs/heads/two\r\n",
        )
        .expect("write packed refs");

        let mut lines = Vec::new();
        for_each_packed_ref_line(&repo.path().join(".git"), |line| {
            lines.push(line.to_owned());
            Ok(true)
        })
        .expect("read packed refs");

        assert_eq!(
            lines,
            vec![
                "# pack-refs with: peeled fully-peeled sorted ".to_owned(),
                "1111111111111111111111111111111111111111 refs/heads/one".to_owned(),
                "^2222222222222222222222222222222222222222".to_owned(),
                "3333333333333333333333333333333333333333 refs/heads/two".to_owned(),
            ]
        );
    }

    #[test]
    fn packed_refs_io_uses_explicit_buffer_capacity() {
        let reader_file = tempfile::tempfile().expect("reader temp file");
        let reader = packed_refs_reader(reader_file);
        let mut writer_file = tempfile::tempfile().expect("writer temp file");
        let writer = packed_refs_writer(&mut writer_file);
        let line = packed_ref_line_buffer();

        assert_eq!(reader.capacity(), PACKED_REFS_IO_BUFFER_CAPACITY);
        assert_eq!(writer.capacity(), PACKED_REFS_IO_BUFFER_CAPACITY);
        assert_eq!(line.capacity(), PACKED_REF_LINE_INITIAL_CAPACITY);
    }

    #[test]
    fn packed_ref_line_reader_rejects_unterminated_last_line() {
        let repo = git_init();
        std::fs::write(
            repo.path().join(".git/packed-refs"),
            "1111111111111111111111111111111111111111 refs/heads/one",
        )
        .expect("write packed refs");

        let error = for_each_packed_ref_line(&repo.path().join(".git"), |_| Ok(true))
            .expect_err("unterminated line should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "unterminated line in .git/packed-refs: 1111111111111111111111111111111111111111 refs/heads/one"
        );
    }

    #[test]
    fn reads_refs_with_hash_and_at_characters_allowed_by_stock_git() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"ref chars\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "initial"]);
        git(&repo, ["branch", "#hash-branch"]);
        git(&repo, ["branch", "user@domain"]);
        git(&repo, ["pack-refs", "--all", "--prune"]);

        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let names = refs.list_refs("refs/heads/").expect("list refs");

        assert!(names.contains(&"refs/heads/#hash-branch".to_owned()));
        assert!(names.contains(&"refs/heads/user@domain".to_owned()));
        assert!(validate_ref_name("refs/heads/bad@{name").is_err());
    }

    #[test]
    fn deletes_packed_ref_written_by_stock_git() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"delete packed ref\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "initial"]);
        git(&repo, ["tag", "v1"]);
        git(&repo, ["pack-refs", "--all", "--prune"]);

        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        refs.delete_ref("refs/tags/v1").expect("delete packed tag");

        assert!(refs.resolve("refs/tags/v1").is_err());
        assert!(
            git_expect_failure(&repo, ["rev-parse", "--verify", "refs/tags/v1"])
                .contains("Needed a single revision")
        );
    }

    #[test]
    fn packed_ref_delete_rewrites_file_without_materializing_lines() {
        let repo = git_init();
        let packed_refs = repo.path().join(".git/packed-refs");
        std::fs::write(
            &packed_refs,
            "# pack-refs with: peeled fully-peeled sorted \n\
             1111111111111111111111111111111111111111 refs/heads/main\n\
             2222222222222222222222222222222222222222 refs/tags/delete-me\n\
             ^3333333333333333333333333333333333333333\n\
             4444444444444444444444444444444444444444 refs/tags/keep-me\n",
        )
        .expect("write packed refs");

        let deleted = delete_packed_ref(
            GitHashAlgorithm::Sha1,
            &repo.path().join(".git"),
            "refs/tags/delete-me",
        )
        .expect("delete packed ref");

        let rewritten = std::fs::read_to_string(&packed_refs).expect("read packed refs");
        assert!(deleted);
        assert!(rewritten.contains("refs/heads/main\n"));
        assert!(rewritten.contains("refs/tags/keep-me\n"));
        assert!(!rewritten.contains("refs/tags/delete-me"));
        assert!(!rewritten.contains("^3333333333333333333333333333333333333333"));
    }

    #[test]
    fn packed_ref_delete_missing_ref_does_not_touch_existing_lock() {
        let repo = git_init();
        let packed_refs = repo.path().join(".git/packed-refs");
        std::fs::write(
            &packed_refs,
            "1111111111111111111111111111111111111111 refs/heads/main\n",
        )
        .expect("write packed refs");
        std::fs::write(lock_path(&packed_refs), b"locked").expect("write lock");

        let deleted = delete_packed_ref(
            GitHashAlgorithm::Sha1,
            &repo.path().join(".git"),
            "refs/heads/missing",
        )
        .expect("missing packed ref");

        assert!(!deleted);
        assert_eq!(
            std::fs::read(lock_path(&packed_refs)).expect("read lock"),
            b"locked"
        );
    }

    #[test]
    fn packs_refs_with_peeled_tags_readable_by_stock_git() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"pack refs\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "initial"]);
        git(&repo, ["branch", "feature"]);
        git(&repo, ["tag", "lightweight"]);
        git_env(&repo, ["tag", "-a", "annotated", "-m", "tag message"]);

        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        refs.pack_refs(PackRefsOptions {
            all: true,
            prune: true,
            auto: false,
            include: Vec::new(),
            exclude: Vec::new(),
        })
        .expect("pack refs");

        let packed_refs = std::fs::read_to_string(repo.path().join(".git/packed-refs"))
            .expect("read packed refs");
        assert!(packed_refs.contains(" refs/heads/feature\n"));
        assert!(packed_refs.contains(" refs/tags/lightweight\n"));
        assert!(packed_refs.contains(" refs/tags/annotated\n^"));
        assert_eq!(
            git(&repo, ["rev-parse", "feature"]),
            git(&repo, ["rev-parse", "HEAD"])
        );
        assert_eq!(
            git(&repo, ["rev-parse", "annotated^{}"]),
            git(&repo, ["rev-parse", "HEAD"])
        );
    }

    #[test]
    fn pack_refs_skips_invalid_peeled_value_for_corrupt_tag() {
        let repo = git_init();
        std::fs::write(repo.path().join("blob-content"), b"garbage\n").expect("write blob source");
        git(&repo, ["hash-object", "-w", "-t", "blob", "blob-content"]);
        let blob_id = git(&repo, ["hash-object", "blob-content"]);
        std::fs::write(
            repo.path().join("tag-content"),
            format!(
                "object {blob_id}\n\
                 type commit\n\
                 tag bad-tag\n\
                 tagger C O Mitter <committer@example.com> 1112354055 +0200\n\n\
                 annotated\n"
            ),
        )
        .expect("write tag");
        let tag_id = git(&repo, ["hash-object", "-w", "-t", "tag", "tag-content"]);
        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        refs.write_ref(
            "refs/tags/bad-tag",
            &ObjectId::from_hex(GitHashAlgorithm::Sha1, &tag_id).expect("tag id"),
        )
        .expect("write tag ref");

        refs.pack_refs(PackRefsOptions {
            all: true,
            prune: false,
            auto: false,
            include: Vec::new(),
            exclude: Vec::new(),
        })
        .expect("pack refs");

        let packed_refs = std::fs::read_to_string(repo.path().join(".git/packed-refs"))
            .expect("read packed refs");
        assert!(packed_refs.contains(" refs/tags/bad-tag\n"));
        assert!(!packed_refs.contains('^'));
    }

    #[test]
    fn pack_refs_auto_uses_loose_ref_thresholds() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"auto pack refs\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "initial"]);
        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let head = ObjectId::from_hex(GitHashAlgorithm::Sha1, &git(&repo, ["rev-parse", "HEAD"]))
            .expect("head id");

        for index in 1..=14 {
            refs.write_ref(&format!("refs/heads/loose-{index}"), &head)
                .expect("write loose ref");
        }
        refs.pack_refs(PackRefsOptions {
            all: true,
            prune: true,
            auto: true,
            include: Vec::new(),
            exclude: Vec::new(),
        })
        .expect("auto pack refs below threshold");
        assert!(!repo.path().join(".git/packed-refs").exists());

        refs.write_ref("refs/heads/loose-15", &head)
            .expect("write threshold ref");
        refs.pack_refs(PackRefsOptions {
            all: true,
            prune: true,
            auto: true,
            include: Vec::new(),
            exclude: Vec::new(),
        })
        .expect("auto pack refs at threshold");
        assert!(repo.path().join(".git/packed-refs").is_file());
    }

    #[test]
    fn peels_nested_tags_from_in_memory_object_store() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"tag target\n")
            .expect("write blob");
        let tagger = Signature::new("Zmin Test", "zmin@example.invalid", 1_700_000_002, "+0000")
            .expect("signature");
        let first_tag = TagBuilder::new(blob.clone(), GitObjectKind::Blob, "v1", tagger.clone())
            .expect("first tag")
            .message(b"first\n".to_vec())
            .expect("first message")
            .encode()
            .expect("encode first tag");
        let first_tag_id = store
            .write_object(GitObjectKind::Tag, &first_tag)
            .expect("write first tag");
        let second_tag = TagBuilder::new(first_tag_id, GitObjectKind::Tag, "v2", tagger)
            .expect("second tag")
            .message(b"second\n".to_vec())
            .expect("second message")
            .encode()
            .expect("encode second tag");
        let second_tag_id = store
            .write_object(GitObjectKind::Tag, &second_tag)
            .expect("write second tag");

        let peeled = peel_tag_ref(&store, &second_tag_id).expect("peel tag");

        assert_eq!(peeled, Some(blob));
    }

    #[test]
    fn server_info_refs_match_stock_git_info_refs() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"server info refs\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "initial"]);
        git(&repo, ["branch", "feature"]);
        git(&repo, ["tag", "lightweight"]);
        git_env(&repo, ["tag", "-a", "annotated", "-m", "tag message"]);

        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let actual = refs
            .server_info_refs()
            .expect("server info refs")
            .into_iter()
            .map(|row| format!("{}\t{}", row.id.to_hex(), row.name))
            .collect::<Vec<_>>()
            .join("\n");
        git(&repo, ["update-server-info"]);
        let expected =
            std::fs::read_to_string(repo.path().join(".git/info/refs")).expect("read info refs");

        assert_eq!(format!("{actual}\n"), expected);
    }

    #[test]
    fn server_info_refs_use_loose_ref_over_packed_ref_without_repeated_scans() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"base\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "base"]);
        git(&repo, ["branch", "feature"]);
        git(&repo, ["pack-refs", "--all", "--prune"]);
        let packed_feature = git(&repo, ["rev-parse", "refs/heads/feature"]);

        git(&repo, ["checkout", "feature"]);
        std::fs::write(repo.path().join("README.md"), b"feature\n").expect("write file");
        git_env(&repo, ["commit", "-am", "feature"]);
        let loose_feature = git(&repo, ["rev-parse", "refs/heads/feature"]);
        assert_ne!(loose_feature, packed_feature);

        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let feature = refs
            .server_info_refs()
            .expect("server info refs")
            .into_iter()
            .find(|row| row.name == "refs/heads/feature")
            .expect("feature row");

        assert_eq!(feature.id.to_hex(), loose_feature);
    }

    #[test]
    fn server_info_refs_ignore_symbolic_refs_with_missing_targets() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"base\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "base"]);
        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        std::fs::create_dir_all(repo.path().join(".git/refs/remotes/origin"))
            .expect("create remote refs");
        refs.write_symbolic_ref("refs/remotes/origin/HEAD", "refs/remotes/origin/missing")
            .expect("write broken symbolic ref");

        let rows = refs.server_info_refs().expect("server info refs");
        assert!(
            !rows
                .iter()
                .any(|row| row.name == "refs/remotes/origin/HEAD"),
            "broken symbolic refs should be skipped"
        );
    }

    #[test]
    fn resolved_ref_iterator_uses_loose_ref_over_packed_ref() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"base\n").expect("write file");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "base"]);
        git(&repo, ["branch", "feature"]);
        git(&repo, ["pack-refs", "--all", "--prune"]);
        let packed_feature = git(&repo, ["rev-parse", "refs/heads/feature"]);

        git(&repo, ["checkout", "feature"]);
        std::fs::write(repo.path().join("README.md"), b"feature\n").expect("write file");
        git_env(&repo, ["commit", "-am", "feature"]);
        let loose_feature = git(&repo, ["rev-parse", "refs/heads/feature"]);
        assert_ne!(loose_feature, packed_feature);

        let refs = RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1);
        let mut rows = Vec::new();
        refs.for_each_resolved_ref("refs/heads/", |name, id| {
            rows.push((name.to_owned(), id.to_hex()));
            Ok::<(), io::Error>(())
        })
        .expect("resolved refs");

        assert!(rows.contains(&("refs/heads/feature".to_owned(), loose_feature)));
        assert!(!rows.contains(&("refs/heads/feature".to_owned(), packed_feature)));
    }

    #[test]
    fn delete_packed_ref_fails_when_packed_refs_new_lock_exists() {
        let repo = git_init();
        let git_dir = repo.path().join(".git");
        let id = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        std::fs::write(
            git_dir.join("packed-refs"),
            format!("{} refs/remotes/origin/extrabranch\n", id.to_hex()),
        )
        .expect("write packed refs");
        std::fs::write(git_dir.join("packed-refs.new"), b"").expect("write lock");
        let refs = RefStore::new(&git_dir, GitHashAlgorithm::Sha1);

        let error = refs
            .delete_ref("refs/remotes/origin/extrabranch")
            .expect_err("delete should fail on existing packed-refs.new");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(refs.read_ref("refs/remotes/origin/extrabranch").is_ok());
    }

    fn git_init() -> TempDir {
        stock_git_support::git_init()
    }

    fn git_init_reftable() -> TempDir {
        stock_git_support::git_init_reftable()
    }

    fn configured_reftable_store(repo: &TempDir) -> RefStore {
        std::fs::write(
            repo.path().join(".git/config"),
            "[core]\n\trepositoryformatversion = 1\n[extensions]\n\trefStorage = reftable\n",
        )
        .expect("configure reftable");
        RefStore::new(repo.path().join(".git"), GitHashAlgorithm::Sha1)
    }

    fn reftable_table_count(repo: &TempDir) -> usize {
        std::fs::read_to_string(repo.path().join(".git/reftable/tables.list"))
            .expect("read reftable table list")
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
    }

    fn git<const N: usize>(repo: &TempDir, args: [&str; N]) -> String {
        stock_git_support::git(repo, &args)
    }

    fn git_expect_failure<const N: usize>(repo: &TempDir, args: [&str; N]) -> String {
        stock_git_support::git_expect_failure(repo, &args)
    }

    fn git_env<const N: usize>(repo: &TempDir, args: [&str; N]) {
        stock_git_support::git_env(repo, &args);
    }
}
