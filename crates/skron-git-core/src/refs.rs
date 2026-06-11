use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use crate::{
    GitHashAlgorithm, GitObjectKind, GitObjectStore, LooseObjectStore, ObjectId, decode_tag,
};

const PACKED_REFS_IO_BUFFER_CAPACITY: usize = 64 * 1024;
const PACKED_REF_LINE_INITIAL_CAPACITY: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefTarget {
    Direct(ObjectId),
    Symbolic(String),
}

#[derive(Debug, Clone)]
pub struct RefStore {
    git_dir: PathBuf,
    algorithm: GitHashAlgorithm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackRefsOptions {
    pub all: bool,
    pub prune: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfoRef {
    pub id: ObjectId,
    pub name: String,
}

impl RefStore {
    pub fn new(git_dir: impl Into<PathBuf>, algorithm: GitHashAlgorithm) -> Self {
        Self {
            git_dir: git_dir.into(),
            algorithm,
        }
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub fn write_ref(&self, name: &str, id: &ObjectId) -> io::Result<()> {
        validate_ref_name(name)?;
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match ref store",
            ));
        }
        atomic_write(self.ref_path(name), format!("{}\n", id.to_hex()).as_bytes())
    }

    pub fn write_symbolic_ref(&self, name: &str, target: &str) -> io::Result<()> {
        if name == "HEAD" {
            return self.write_head_symbolic(target);
        }
        validate_ref_name(name)?;
        validate_ref_name(target)?;
        atomic_write(self.ref_path(name), format!("ref: {target}\n").as_bytes())
    }

    pub fn delete_ref(&self, name: &str) -> io::Result<()> {
        validate_ref_name(name)?;
        let mut deleted = false;
        match fs::remove_file(self.ref_path(name)) {
            Ok(()) => deleted = true,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        if delete_packed_ref(self.algorithm, &self.git_dir, name)? {
            deleted = true;
        }
        if deleted {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "ref not found"))
        }
    }

    pub fn read_ref(&self, name: &str) -> io::Result<RefTarget> {
        validate_ref_name(name)?;
        match fs::read_to_string(self.ref_path(name)) {
            Ok(raw) => parse_ref_target(self.algorithm, raw.trim_end_matches('\n')),
            Err(err) if err.kind() == io::ErrorKind::NotFound => self
                .read_packed_ref(name)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "ref not found")),
            Err(err) => Err(err),
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

    pub fn for_each_ref_name<E, F>(&self, prefix: &str, mut on_ref: F) -> std::result::Result<(), E>
    where
        E: From<io::Error>,
        F: FnMut(&str) -> std::result::Result<(), E>,
    {
        validate_ref_prefix(prefix).map_err(E::from)?;
        let mut refs = BTreeSet::new();
        collect_loose_refs(&self.git_dir, prefix, &mut refs).map_err(E::from)?;
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
        let packed_refs = self.read_packed_refs()?;
        let packed_names = packed_refs.keys().cloned().collect::<BTreeSet<_>>();
        let mut refs = packed_refs;
        let mut prune_names = Vec::new();

        for name in self.loose_ref_names("refs/")? {
            let should_pack =
                options.all || name.starts_with("refs/tags/") || packed_names.contains(&name);
            if !should_pack {
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
        atomic_write(self.git_dir.join("packed-refs"), out.as_bytes())?;

        if options.prune {
            for name in prune_names {
                match fs::remove_file(self.ref_path(&name)) {
                    Ok(()) => {}
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err),
                }
            }
        }
        Ok(())
    }

    pub fn write_fresh_packed_refs(
        &self,
        direct_refs: &[(String, ObjectId)],
        symbolic_refs: &[(String, String)],
    ) -> io::Result<()> {
        let mut packed = BTreeMap::new();
        for (name, id) in direct_refs {
            validate_ref_name(name)?;
            if id.algorithm() != self.algorithm {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "object id algorithm does not match ref store",
                ));
            }
            packed.insert(name.clone(), id.clone());
        }
        for (name, target) in symbolic_refs {
            if name == "HEAD" {
                validate_ref_name(target)?;
            } else {
                validate_ref_name(name)?;
                validate_ref_name(target)?;
            }
        }

        if !packed.is_empty() {
            let mut out = String::from("# pack-refs with: sorted\n");
            for (name, id) in packed {
                out.push_str(&id.to_hex());
                out.push(' ');
                out.push_str(&name);
                out.push('\n');
            }
            atomic_write(self.git_dir.join("packed-refs"), out.as_bytes())?;
        }

        for (name, target) in symbolic_refs {
            self.write_symbolic_ref(name, target)?;
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
        self.for_each_resolved_ref::<E, _>("refs/", |name, id| -> std::result::Result<(), E> {
            on_ref(id, name)?;
            if let Some(peeled) = peel_tag_ref(&store, id).map_err(E::from)? {
                on_ref(&peeled, &format!("{name}^{{}}"))?;
            }
            Ok(())
        })?;
        Ok(())
    }

    pub fn write_head_symbolic(&self, target: &str) -> io::Result<()> {
        validate_ref_name(target)?;
        atomic_write(
            self.git_dir.join("HEAD"),
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
        atomic_write(
            self.git_dir.join("HEAD"),
            format!("{}\n", id.to_hex()).as_bytes(),
        )
    }

    pub fn read_head(&self) -> io::Result<RefTarget> {
        let raw = fs::read_to_string(self.git_dir.join("HEAD"))?;
        parse_ref_target(self.algorithm, raw.trim_end_matches('\n'))
    }

    pub fn resolve(&self, name: &str) -> io::Result<ObjectId> {
        match if name == "HEAD" {
            self.read_head()?
        } else {
            self.read_ref(name)?
        } {
            RefTarget::Direct(id) => Ok(id),
            RefTarget::Symbolic(target) => match self.read_ref(&target)? {
                RefTarget::Direct(id) => Ok(id),
                RefTarget::Symbolic(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "nested symbolic refs are not supported yet",
                )),
            },
        }
    }

    fn ref_path(&self, name: &str) -> PathBuf {
        self.git_dir.join(name)
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
        for_each_packed_ref_line(&self.git_dir, |line| {
            if let Some((id, ref_name)) = parse_packed_ref_line(self.algorithm, line)? {
                return on_ref(id, &ref_name);
            }
            Ok(true)
        })
    }

    fn resolved_refs(&self, prefix: &str) -> io::Result<BTreeMap<String, ObjectId>> {
        validate_ref_prefix(prefix)?;
        let mut refs = BTreeMap::new();
        self.for_each_packed_ref(|id, name| {
            if name.starts_with(prefix) {
                refs.insert(name.to_owned(), id);
            }
            Ok(true)
        })?;
        for name in self.loose_ref_names(prefix)? {
            refs.insert(name.clone(), self.resolve(&name)?);
        }
        Ok(refs)
    }

    fn loose_ref_names(&self, prefix: &str) -> io::Result<Vec<String>> {
        validate_ref_prefix(prefix)?;
        let mut refs = BTreeSet::new();
        collect_loose_refs(&self.git_dir, prefix, &mut refs)?;
        Ok(refs.into_iter().collect())
    }
}

fn peel_tag_ref<S: GitObjectStore>(store: &S, id: &ObjectId) -> io::Result<Option<ObjectId>> {
    let object = store.read_object(id)?;
    if object.kind != GitObjectKind::Tag {
        return Ok(None);
    }
    let mut tag = decode_tag(id.algorithm(), &object.content)?;
    loop {
        if tag.target_kind != GitObjectKind::Tag {
            return Ok(Some(tag.target));
        }
        let next = store.read_object(&tag.target)?;
        tag = decode_tag(id.algorithm(), &next.content)?;
    }
}

fn parse_ref_target(algorithm: GitHashAlgorithm, value: &str) -> io::Result<RefTarget> {
    if let Some(target) = value.strip_prefix("ref: ") {
        validate_ref_name(target)?;
        return Ok(RefTarget::Symbolic(target.to_string()));
    }
    ObjectId::from_hex(algorithm, value).map(RefTarget::Direct)
}

pub fn check_ref_format(name: &str, allow_onelevel: bool) -> bool {
    validate_ref_format(name, allow_onelevel).is_ok()
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

fn collect_loose_refs(git_dir: &Path, prefix: &str, refs: &mut BTreeSet<String>) -> io::Result<()> {
    let root = git_dir.join(prefix);
    if !root.exists() {
        return Ok(());
    }
    collect_loose_refs_from_dir(&root, prefix.trim_end_matches('/'), refs)
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
            validate_ref_name(&ref_name)?;
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
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
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
    let file = fs::File::open(&path)?;
    let lock_path = lock_path(&path);
    match write_packed_refs_without_ref(algorithm, file, &lock_path, name) {
        Ok(()) => match replace_with_lock(&lock_path, &path) {
            Ok(()) => Ok(true),
            Err(error) => {
                let _ = fs::remove_file(&lock_path);
                Err(error)
            }
        },
        Err(error) => {
            let _ = fs::remove_file(&lock_path);
            Err(error)
        }
    }
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
            writer.write_all(line.as_bytes())?;
            writer.write_all(b"\n")?;
            line.clear();
        }
        writer.flush()?;
    }
    lock.sync_all()?;
    Ok(())
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
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "packed ref missing ref name"))?;
    if parts.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "packed ref has trailing fields",
        ));
    }
    validate_ref_name(name)?;
    ObjectId::from_hex(algorithm, id).map(|id| Some((id, name.to_owned())))
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
    lock.write_all(bytes)?;
    lock.sync_all()
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
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
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
             3333333333333333333333333333333333333333 refs/heads/two",
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
    fn peels_nested_tags_from_in_memory_object_store() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"tag target\n")
            .expect("write blob");
        let tagger = Signature::new(
            "Skron Test",
            "skron@example.invalid",
            1_700_000_002,
            "+0000",
        )
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

    fn git_init() -> TempDir {
        let repo = TempDir::new().expect("temp repo");
        let output = Command::new("git")
            .arg("init")
            .arg("--quiet")
            .current_dir(repo.path())
            .output()
            .expect("run git init");
        assert!(
            output.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        repo
    }

    fn git<const N: usize>(repo: &TempDir, args: [&str; N]) -> String {
        let output = Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(repo.path())
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git stdout utf8")
            .trim_end_matches('\n')
            .to_owned()
    }

    fn git_expect_failure<const N: usize>(repo: &TempDir, args: [&str; N]) -> String {
        let output = Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(repo.path())
            .output()
            .expect("run git");
        assert!(
            !output.status.success(),
            "git unexpectedly succeeded: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        String::from_utf8(output.stderr)
            .expect("git stderr utf8")
            .trim_end_matches('\n')
            .to_owned()
    }

    fn git_env<const N: usize>(repo: &TempDir, args: [&str; N]) {
        let output = Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(repo.path())
            .env("GIT_AUTHOR_NAME", "Skron")
            .env("GIT_AUTHOR_EMAIL", "skron@example.com")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_NAME", "Skron")
            .env("GIT_COMMITTER_EMAIL", "skron@example.com")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
