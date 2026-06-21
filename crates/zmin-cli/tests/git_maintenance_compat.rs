mod common;

use std::collections::BTreeSet;
use std::fs;

use tempfile::TempDir;
use zmin_git_core::{GitHashAlgorithm, GitObjectHash};

use common::{
    command_any_output, command_failure_output_with_env, command_output_with_env,
    command_stdout_bytes, configure_identity, git, git_failure_output, git_init, git_status,
    git_with_env, git_with_stdin, git_with_stdin_args, run_zmin, run_zmin_failure_output,
    write_file, zmin_bin,
};

fn pack_refs_fixture_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"a\n").expect("write a");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["branch", "feature"]);
    git(repo.path(), ["tag", "lightweight"]);
    git_with_env(repo.path(), ["tag", "-a", "annotated", "-m", "tag message"]);
    repo
}

#[test]
fn maintenance_unknown_subcommand_failure_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["maintenance", "frobnicate"]),
        git_failure_output(git_repo.path(), &["maintenance", "frobnicate"])
    );
}

fn normalize_repo_path(text: String, repo: &std::path::Path) -> String {
    let canonical = fs::canonicalize(repo)
        .expect("canonical repo path")
        .display()
        .to_string();
    text.replace(&canonical, "<repo>")
        .replace(&git_path_output_string(canonical), "<repo>")
}

#[cfg(windows)]
fn git_path_output_string(value: String) -> String {
    let value = if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        value
    };
    value.replace('\\', "/")
}

#[cfg(not(windows))]
fn git_path_output_string(value: String) -> String {
    value
}

fn read_u32_be(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("u32 bytes"))
}

fn read_u64_be(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(bytes.try_into().expect("u64 bytes"))
}

fn push_u64_be(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn rewrite_midx_first_offset_to_loff(repo: &std::path::Path) {
    let path = repo.join(".git/objects/pack/multi-pack-index");
    let bytes = fs::read(&path).expect("read midx");
    assert_eq!(&bytes[..4], b"MIDX");
    let digest_len = GitHashAlgorithm::Sha1.digest_len();
    let graph_end = bytes.len() - digest_len;
    let chunk_count = bytes[6] as usize;
    let lookup_end = 12 + (chunk_count + 1) * 12;
    let mut chunks = Vec::new();
    for idx in 0..chunk_count {
        let cursor = 12 + idx * 12;
        let id = bytes[cursor..cursor + 4].to_vec();
        let start = read_u64_be(&bytes[cursor + 4..cursor + 12]) as usize;
        let end = read_u64_be(&bytes[cursor + 16..cursor + 24]) as usize;
        chunks.push((id, bytes[start..end].to_vec()));
    }
    assert!(lookup_end <= graph_end);

    let ooff = chunks
        .iter_mut()
        .find(|(id, _)| id == b"OOFF")
        .expect("OOFF chunk");
    let original_offset = read_u32_be(&ooff.1[4..8]);
    ooff.1[4..8].copy_from_slice(&0x8000_0000_u32.to_be_bytes());

    let mut loff = Vec::new();
    push_u64_be(&mut loff, u64::from(original_offset));
    chunks.push((b"LOFF".to_vec(), loff));

    let mut out = Vec::new();
    out.extend_from_slice(&bytes[..6]);
    out.push(u8::try_from(chunk_count + 1).expect("chunk count"));
    out.push(bytes[7]);
    out.extend_from_slice(&bytes[8..12]);

    let mut offset = 12 + (chunks.len() as u64 + 1) * 12;
    for (id, data) in &chunks {
        out.extend_from_slice(id);
        push_u64_be(&mut out, offset);
        offset += data.len() as u64;
    }
    out.extend_from_slice(&[0, 0, 0, 0]);
    push_u64_be(&mut out, offset);
    for (_, data) in &chunks {
        out.extend_from_slice(data);
    }

    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&out);
    out.extend_from_slice(hasher.finalize().as_bytes());
    fs::write(path, out).expect("write midx with LOFF");
}

fn rewrite_midx_header(repo: &std::path::Path, offset: usize, replacement: &[u8]) {
    let path = repo.join(".git/objects/pack/multi-pack-index");
    let mut bytes = fs::read(&path).expect("read midx");
    assert_eq!(&bytes[..4], b"MIDX");
    let digest_len = GitHashAlgorithm::Sha1.digest_len();
    let checksum_start = bytes.len() - digest_len;
    assert!(offset + replacement.len() <= checksum_start);
    bytes[offset..offset + replacement.len()].copy_from_slice(replacement);

    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&bytes[..checksum_start]);
    let checksum = hasher.finalize();
    bytes[checksum_start..].copy_from_slice(checksum.as_bytes());
    fs::write(path, bytes).expect("write rewritten midx header");
}

fn duplicate_packed_head_as_loose(repo: &std::path::Path) -> String {
    let id = git(repo, ["rev-parse", "HEAD"]);
    let loose_path = loose_object_path(repo, &id);
    let copy_path = repo.join("duplicate-head-copy");
    fs::copy(&loose_path, &copy_path).expect("copy loose object");
    git(repo, ["repack", "-adq"]);
    fs::create_dir_all(loose_path.parent().expect("loose object parent"))
        .expect("create loose object dir");
    fs::copy(copy_path, &loose_path).expect("restore duplicate loose object");
    assert!(loose_object_exists(repo, &id));
    id
}

fn prune_fixture(repo: &std::path::Path) -> (String, String, String) {
    configure_identity(repo);
    git_with_env(repo, ["commit", "--allow-empty", "-m", "base"]);
    write_file(repo, "staged.txt", "staged\n");
    git(repo, ["add", "staged.txt"]);
    let staged = git(repo, ["hash-object", "staged.txt"]);
    let pruned = git_with_stdin(repo, ["hash-object", "-w", "--stdin"], "prune me\n");
    let kept = git_with_stdin(repo, ["hash-object", "-w", "--stdin"], "keep me\n");
    (pruned, kept, staged)
}

fn reflog_prune_fixture(repo: &std::path::Path) -> String {
    configure_identity(repo);
    git(repo, ["checkout", "-b", "main"]);
    git_with_env(repo, ["commit", "--allow-empty", "-m", "base"]);
    git(repo, ["checkout", "-b", "topic"]);
    write_file(repo, "topic.txt", "topic\n");
    git(repo, ["add", "-A"]);
    git_with_env(repo, ["commit", "-m", "topic"]);
    let topic_commit = git(repo, ["rev-parse", "HEAD"]);
    git(repo, ["checkout", "main"]);
    git(repo, ["branch", "-D", "topic"]);
    topic_commit
}

fn loose_object_exists(repo: &std::path::Path, id: &str) -> bool {
    loose_object_path(repo, id).is_file()
}

fn loose_object_path(repo: &std::path::Path, id: &str) -> std::path::PathBuf {
    repo.join(".git/objects").join(&id[..2]).join(&id[2..])
}

fn first_pack_index(repo: &std::path::Path) -> std::path::PathBuf {
    let mut paths = fs::read_dir(repo.join(".git/objects/pack"))
        .expect("read pack dir")
        .map(|entry| entry.expect("pack entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("idx"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.into_iter().next().expect("pack index")
}

fn pack_file_count(repo: &std::path::Path) -> usize {
    pack_file_names(repo)
        .iter()
        .filter(|name| name.ends_with(".pack"))
        .count()
}

fn verify_pack_delta_base(verify: &str, id: &str) -> Option<String> {
    verify.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != id {
            return None;
        }
        let _kind = fields.next()?;
        let _size = fields.next()?;
        let _packed_size = fields.next()?;
        let _offset = fields.next()?;
        let _depth = fields.next()?;
        Some(fields.next()?.to_owned())
    })
}

fn pack_file_names(repo: &std::path::Path) -> BTreeSet<String> {
    match fs::read_dir(repo.join(".git/objects/pack")) {
        Ok(entries) => entries
            .map(|entry| {
                entry
                    .expect("pack entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
        Err(error) => panic!("read pack dir: {error}"),
    }
}

fn two_pack_midx_fixture() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    let first_objects = git(
        repo.path(),
        ["rev-list", "--objects", "--no-object-names", "HEAD"],
    );
    git_with_stdin_args(
        repo.path(),
        &["pack-objects", ".git/objects/pack/pack-base"],
        &first_objects,
    );

    write_file(repo.path(), "b.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    let second_objects = git(
        repo.path(),
        [
            "rev-list",
            "--objects",
            "--no-object-names",
            "HEAD",
            "^HEAD~1",
        ],
    );
    git_with_stdin_args(
        repo.path(),
        &["pack-objects", ".git/objects/pack/pack-extra"],
        &second_objects,
    );
    repo
}

fn packed_object_ids(repo: &std::path::Path) -> BTreeSet<String> {
    let idx = first_pack_index(repo);
    git(repo, ["verify-pack", "-v", idx.to_str().expect("idx path")])
        .lines()
        .filter_map(|line| {
            let id = line.split_whitespace().next()?;
            (id.len() == 40 && id.as_bytes().iter().all(u8::is_ascii_hexdigit))
                .then(|| id.to_owned())
        })
        .collect()
}

fn ref_file_list(repo: &std::path::Path) -> Vec<String> {
    let mut refs = Vec::new();
    collect_ref_files(&repo.join(".git/refs"), "refs", &mut refs);
    refs.sort();
    refs
}

fn collect_ref_files(dir: &std::path::Path, prefix: &str, refs: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("read ref dir {dir:?}: {error}"),
    };
    for entry in entries {
        let entry = entry.expect("read ref entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let ref_name = format!("{prefix}/{name}");
        if path.is_dir() {
            collect_ref_files(&path, &ref_name, refs);
        } else if path.is_file() {
            refs.push(ref_name);
        }
    }
}

#[test]
fn commit_graph_write_creates_stock_verifiable_graph() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "b.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);

    assert_eq!(
        run_zmin(repo.path(), ["commit-graph", "write", "--reachable"]),
        ""
    );
    assert!(repo.path().join(".git/objects/info/commit-graph").exists());
    assert_eq!(run_zmin(repo.path(), ["commit-graph", "verify"]), "");
    assert_eq!(git_status(repo.path(), ["commit-graph", "verify"]), 0);
}

#[test]
fn commit_graph_write_handles_octopus_merge_edge_chunk() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "base.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);

    for branch in ["one", "two", "three"] {
        git(repo.path(), ["checkout", "-b", branch, "main"]);
        write_file(
            repo.path(),
            &format!("{branch}.txt"),
            &format!("{branch}\n"),
        );
        git(repo.path(), ["add", "-A"]);
        git_with_env(repo.path(), ["commit", "-m", branch]);
    }
    git(repo.path(), ["checkout", "main"]);
    git_with_env(
        repo.path(),
        ["merge", "--no-ff", "-m", "octopus", "one", "two", "three"],
    );
    assert_eq!(
        git(repo.path(), ["rev-list", "--parents", "-n", "1", "HEAD"])
            .split_whitespace()
            .count(),
        5
    );

    assert_eq!(
        run_zmin(repo.path(), ["commit-graph", "write", "--reachable"]),
        ""
    );
    let graph =
        fs::read(repo.path().join(".git/objects/info/commit-graph")).expect("read commit graph");
    assert!(
        graph.windows(4).any(|window| window == b"EDGE"),
        "octopus commit graph should include EDGE chunk"
    );
    assert_eq!(run_zmin(repo.path(), ["commit-graph", "verify"]), "");
    assert_eq!(git_status(repo.path(), ["commit-graph", "verify"]), 0);
}

#[test]
fn multi_pack_index_write_creates_stock_verifiable_index() {
    let repo = two_pack_midx_fixture();

    assert_eq!(run_zmin(repo.path(), ["multi-pack-index", "write"]), "");
    assert!(
        repo.path()
            .join(".git/objects/pack/multi-pack-index")
            .exists()
    );
    assert_eq!(run_zmin(repo.path(), ["multi-pack-index", "verify"]), "");
    assert_eq!(git_status(repo.path(), ["multi-pack-index", "verify"]), 0);
}

#[test]
fn multi_pack_index_verify_accepts_large_offset_chunk() {
    let repo = two_pack_midx_fixture();

    assert_eq!(run_zmin(repo.path(), ["multi-pack-index", "write"]), "");
    rewrite_midx_first_offset_to_loff(repo.path());

    let midx = fs::read(repo.path().join(".git/objects/pack/multi-pack-index"))
        .expect("read rewritten midx");
    assert!(
        midx.windows(4).any(|window| window == b"LOFF"),
        "rewritten midx should contain LOFF"
    );
    assert_eq!(run_zmin(repo.path(), ["multi-pack-index", "verify"]), "");
    assert_eq!(git_status(repo.path(), ["multi-pack-index", "verify"]), 0);
}

#[test]
fn multi_pack_index_verify_header_variants_match_stock_git() {
    for (label, offset, replacement) in [
        ("bad signature", 0, b"BAD!".as_slice()),
        ("bad version", 4, b"\x02".as_slice()),
        ("bad hash version", 5, b"\x02".as_slice()),
        ("reserved byte", 7, b"\x01".as_slice()),
    ] {
        let git_repo = two_pack_midx_fixture();
        let zmin_repo = two_pack_midx_fixture();
        git(git_repo.path(), ["multi-pack-index", "write"]);
        git(zmin_repo.path(), ["multi-pack-index", "write"]);
        rewrite_midx_header(git_repo.path(), offset, replacement);
        rewrite_midx_header(zmin_repo.path(), offset, replacement);

        assert_eq!(
            command_any_output(
                zmin_bin(),
                zmin_repo.path(),
                &["multi-pack-index", "verify"],
                "zmin",
            ),
            command_any_output(
                "git",
                git_repo.path(),
                &["multi-pack-index", "verify"],
                "git",
            ),
            "{label}"
        );
    }
}

#[test]
fn multi_pack_index_write_empty_repo_failure_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["multi-pack-index", "write"]),
        git_failure_output(git_repo.path(), &["multi-pack-index", "write"])
    );
    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &["multi-pack-index", "write", "--no-progress"]
        ),
        git_failure_output(
            git_repo.path(),
            &["multi-pack-index", "write", "--no-progress"]
        )
    );
}

#[test]
fn repack_write_midx_empty_repo_noops_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin(zmin_repo.path(), ["repack", "-m", "-q"]),
        git(git_repo.path(), ["repack", "-m", "-q"])
    );
    assert_eq!(
        zmin_repo
            .path()
            .join(".git/objects/pack/multi-pack-index")
            .exists(),
        git_repo
            .path()
            .join(".git/objects/pack/multi-pack-index")
            .exists()
    );
}

#[test]
fn multi_pack_index_progress_flags_are_accepted_like_stock_git() {
    let repo = two_pack_midx_fixture();

    assert_eq!(
        run_zmin(repo.path(), ["multi-pack-index", "write", "--no-progress"]),
        ""
    );
    assert_eq!(
        run_zmin(repo.path(), ["multi-pack-index", "verify", "--progress"]),
        ""
    );
    assert_eq!(
        run_zmin(repo.path(), ["multi-pack-index", "expire", "--no-progress"]),
        ""
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["multi-pack-index", "repack", "--progress", "--batch-size=1"]
        ),
        ""
    );
    assert_eq!(git_status(repo.path(), ["multi-pack-index", "verify"]), 0);
}

#[test]
fn multi_pack_index_repack_batch_size_one_noops_like_stock_git() {
    let git_repo = two_pack_midx_fixture();
    let zmin_repo = two_pack_midx_fixture();
    git(git_repo.path(), ["multi-pack-index", "write"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["multi-pack-index", "write"]),
        ""
    );
    let git_before = pack_file_names(git_repo.path());
    let zmin_before = pack_file_names(zmin_repo.path());

    git(
        git_repo.path(),
        ["multi-pack-index", "repack", "--batch-size=1"],
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["multi-pack-index", "repack", "--batch-size=1"]
        ),
        ""
    );
    assert_eq!(pack_file_names(git_repo.path()), git_before);
    assert_eq!(pack_file_names(zmin_repo.path()), zmin_before);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["multi-pack-index", "verify"]),
        ""
    );
    assert_eq!(
        git_status(zmin_repo.path(), ["multi-pack-index", "verify"]),
        0
    );
}

#[test]
fn multi_pack_index_repack_and_expire_consolidate_like_stock_git() {
    let git_repo = two_pack_midx_fixture();
    let zmin_repo = two_pack_midx_fixture();
    git(git_repo.path(), ["multi-pack-index", "write"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["multi-pack-index", "write"]),
        ""
    );

    git(
        git_repo.path(),
        ["multi-pack-index", "repack", "--batch-size=0"],
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["multi-pack-index", "repack", "--batch-size=0"]
        ),
        ""
    );
    assert_eq!(
        pack_file_count(zmin_repo.path()),
        pack_file_count(git_repo.path())
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["multi-pack-index", "verify"]),
        ""
    );
    assert_eq!(
        git_status(zmin_repo.path(), ["multi-pack-index", "verify"]),
        0
    );

    git(git_repo.path(), ["multi-pack-index", "expire"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["multi-pack-index", "expire"]),
        ""
    );
    assert_eq!(
        pack_file_count(zmin_repo.path()),
        pack_file_count(git_repo.path())
    );
    assert_eq!(pack_file_count(zmin_repo.path()), 1);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["multi-pack-index", "verify"]),
        ""
    );
    assert_eq!(
        git_status(zmin_repo.path(), ["multi-pack-index", "verify"]),
        0
    );
    assert_eq!(git_status(zmin_repo.path(), ["fsck", "--strict"]), 0);
}

#[test]
fn pack_refs_matches_stock_git_for_default_all_and_no_prune() {
    let git_repo = pack_refs_fixture_repo();
    let zmin_repo = pack_refs_fixture_repo();

    git(git_repo.path(), ["pack-refs"]);
    run_zmin(zmin_repo.path(), ["pack-refs"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/packed-refs"))
            .expect("read zmin packed refs"),
        fs::read_to_string(git_repo.path().join(".git/packed-refs")).expect("read git packed refs")
    );
    assert_eq!(
        ref_file_list(zmin_repo.path()),
        ref_file_list(git_repo.path())
    );

    let git_repo = pack_refs_fixture_repo();
    let zmin_repo = pack_refs_fixture_repo();
    git(git_repo.path(), ["pack-refs", "--all", "--no-prune"]);
    run_zmin(zmin_repo.path(), ["pack-refs", "--all", "--no-prune"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/packed-refs"))
            .expect("read zmin packed refs"),
        fs::read_to_string(git_repo.path().join(".git/packed-refs")).expect("read git packed refs")
    );
    assert_eq!(
        ref_file_list(zmin_repo.path()),
        ref_file_list(git_repo.path())
    );

    let git_repo = pack_refs_fixture_repo();
    let zmin_repo = pack_refs_fixture_repo();
    git(git_repo.path(), ["pack-refs", "--all", "--prune"]);
    run_zmin(zmin_repo.path(), ["pack-refs", "--all", "--prune"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/packed-refs"))
            .expect("read zmin packed refs"),
        fs::read_to_string(git_repo.path().join(".git/packed-refs")).expect("read git packed refs")
    );
    assert_eq!(
        ref_file_list(zmin_repo.path()),
        ref_file_list(git_repo.path())
    );
}

#[test]
fn prune_packed_matches_stock_git_for_dry_run_and_prune() {
    let git_repo = pack_refs_fixture_repo();
    let zmin_repo = pack_refs_fixture_repo();
    let git_head = duplicate_packed_head_as_loose(git_repo.path());
    let zmin_head = duplicate_packed_head_as_loose(zmin_repo.path());
    assert_eq!(git_head, zmin_head);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["prune-packed", "-n"]),
        git(git_repo.path(), ["prune-packed", "-n"])
    );
    assert!(loose_object_exists(zmin_repo.path(), &zmin_head));

    git(git_repo.path(), ["prune-packed"]);
    run_zmin(zmin_repo.path(), ["prune-packed"]);
    assert_eq!(
        loose_object_exists(zmin_repo.path(), &zmin_head),
        loose_object_exists(git_repo.path(), &git_head)
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-t", &zmin_head]),
        git(git_repo.path(), ["cat-file", "-t", &git_head])
    );
}

#[test]
fn repack_packs_objects_and_prunes_loose_duplicates() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    write_file(repo.path(), "b.txt", "bee\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    let head = git(repo.path(), ["rev-parse", "HEAD"]);
    let objects = git(repo.path(), ["rev-list", "--objects", "HEAD"])
        .lines()
        .map(|line| {
            line.split_whitespace()
                .next()
                .expect("object id")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(
        objects
            .iter()
            .any(|id| loose_object_exists(repo.path(), id))
    );

    assert_eq!(run_zmin(repo.path(), ["repack", "-adq"]), "");
    assert_eq!(run_zmin(repo.path(), ["repack", "-adq"]), "");
    assert!(first_pack_index(repo.path()).is_file());
    for id in &objects {
        assert!(
            !loose_object_exists(repo.path(), id),
            "loose object should be pruned after repack: {id}"
        );
        assert!(!git(repo.path(), ["cat-file", "-t", id]).is_empty());
    }
    assert!(git(repo.path(), ["cat-file", "-p", &head]).contains("\n\ntwo"));
    assert!(
        fs::read_to_string(repo.path().join(".git/objects/info/packs"))
            .expect("read packs info")
            .contains("P pack-")
    );
}

#[test]
fn incremental_repack_delete_redundant_keeps_existing_pack_objects() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "master"]);

    write_file(repo.path(), "first.t", "first\n");
    git(repo.path(), ["add", "--", "first.t"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    git(repo.path(), ["tag", "first"]);
    let first = git(repo.path(), ["rev-parse", "refs/tags/first"]);
    assert_eq!(run_zmin(repo.path(), ["repack", "-Adq"]), "");
    assert_eq!(run_zmin(repo.path(), ["cat-file", "-t", &first]), "commit");

    write_file(repo.path(), "second.t", "second\n");
    git(repo.path(), ["add", "--", "second.t"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    git(repo.path(), ["tag", "second"]);
    assert_eq!(run_zmin(repo.path(), ["repack", "-dq"]), "");

    assert_eq!(run_zmin(repo.path(), ["cat-file", "-t", &first]), "commit");
    assert!(pack_file_count(repo.path()) >= 2);
}

#[test]
fn repack_all_keeps_unreachable_loose_objects_out_of_pack() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "a.txt", "one\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "one"]);
    }
    let git_dangling = git_with_stdin(
        git_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "dangling\n",
    );
    let zmin_dangling = git_with_stdin(
        zmin_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "dangling\n",
    );
    assert_eq!(zmin_dangling, git_dangling);

    git(git_repo.path(), ["repack", "-adq"]);
    assert_eq!(run_zmin(zmin_repo.path(), ["repack", "-adq"]), "");
    assert!(loose_object_exists(zmin_repo.path(), &zmin_dangling));
    assert_eq!(
        loose_object_exists(zmin_repo.path(), &zmin_dangling),
        loose_object_exists(git_repo.path(), &git_dangling)
    );
    assert_eq!(
        packed_object_ids(zmin_repo.path()).contains(&zmin_dangling),
        packed_object_ids(git_repo.path()).contains(&git_dangling)
    );
}

#[test]
fn repack_loosens_packed_unreachable_objects_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let mut unreachable_commits = Vec::new();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        git(repo, ["checkout", "-b", "main"]);
        write_file(repo, "a.txt", "base\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
        git(repo, ["checkout", "-b", "tmpbranch"]);
        write_file(repo, "tmp.txt", "tmp\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "tmp"]);
        unreachable_commits.push(git(repo, ["rev-parse", "HEAD"]));
        git(repo, ["checkout", "main"]);
        git(repo, ["repack", "-adq"]);
        git(repo, ["branch", "-D", "tmpbranch"]);
        git(
            repo,
            [
                "reflog",
                "expire",
                "--expire=now",
                "--expire-unreachable=now",
                "--all",
            ],
        );
    }
    let git_unreachable = &unreachable_commits[0];
    let zmin_unreachable = &unreachable_commits[1];
    assert_eq!(zmin_unreachable, git_unreachable);

    git(git_repo.path(), ["repack", "-Adq"]);
    assert_eq!(run_zmin(zmin_repo.path(), ["repack", "-Adq"]), "");
    assert!(loose_object_exists(zmin_repo.path(), zmin_unreachable));
    assert_eq!(
        loose_object_exists(zmin_repo.path(), zmin_unreachable),
        loose_object_exists(git_repo.path(), git_unreachable)
    );
    assert_eq!(
        packed_object_ids(zmin_repo.path()).contains(zmin_unreachable),
        packed_object_ids(git_repo.path()).contains(git_unreachable)
    );
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-t", zmin_unreachable]),
        git(git_repo.path(), ["cat-file", "-t", git_unreachable])
    );
}

#[test]
fn repack_write_midx_creates_stock_verifiable_multi_pack_index() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);

    assert_eq!(run_zmin(repo.path(), ["repack", "-adqm"]), "");
    assert!(
        repo.path()
            .join(".git/objects/pack/multi-pack-index")
            .is_file()
    );
    assert_eq!(run_zmin(repo.path(), ["multi-pack-index", "verify"]), "");
    assert_eq!(git_status(repo.path(), ["multi-pack-index", "verify"]), 0);

    write_file(repo.path(), "b.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    assert_eq!(run_zmin(repo.path(), ["repack", "-m", "-n", "-q"]), "");
    assert_eq!(git_status(repo.path(), ["multi-pack-index", "verify"]), 0);
}

#[test]
fn repack_negated_bitmap_and_midx_flags_match_stock_git_effects() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "a.txt", "one\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "one"]);
    }

    git(git_repo.path(), ["repack", "-adqm", "--no-write-midx"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["repack", "-adqm", "--no-write-midx"]),
        ""
    );
    assert_eq!(
        zmin_repo
            .path()
            .join(".git/objects/pack/multi-pack-index")
            .exists(),
        git_repo
            .path()
            .join(".git/objects/pack/multi-pack-index")
            .exists()
    );

    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "b.txt", "two\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "two"]);
    }
    git(
        git_repo.path(),
        ["repack", "-adq", "-b", "--no-write-bitmap-index"],
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["repack", "-adq", "-b", "--no-write-bitmap-index"]
        ),
        ""
    );
    assert_eq!(
        git_status(zmin_repo.path(), ["fsck", "--strict"]),
        git_status(git_repo.path(), ["fsck", "--strict"])
    );
}

#[test]
fn repack_delete_redundant_removes_stale_multi_pack_index_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "a.txt", "one\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "one"]);
    }
    assert_eq!(run_zmin(zmin_repo.path(), ["repack", "-adqm"]), "");
    git(git_repo.path(), ["repack", "-adqm"]);
    assert!(
        zmin_repo
            .path()
            .join(".git/objects/pack/multi-pack-index")
            .is_file()
    );

    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "b.txt", "two\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "two"]);
    }
    git(git_repo.path(), ["repack", "-adq"]);
    assert_eq!(run_zmin(zmin_repo.path(), ["repack", "-adq"]), "");
    assert_eq!(
        zmin_repo
            .path()
            .join(".git/objects/pack/multi-pack-index")
            .exists(),
        git_repo
            .path()
            .join(".git/objects/pack/multi-pack-index")
            .exists()
    );
    assert_eq!(
        git_status(zmin_repo.path(), ["multi-pack-index", "verify"]),
        0
    );
}

#[test]
fn repack_keep_pack_preserves_named_pack_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "a.txt", "one\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "one"]);
        git(repo, ["repack", "-q"]);
    }
    let git_keep = first_pack_index(git_repo.path())
        .with_extension("pack")
        .file_name()
        .expect("git keep pack")
        .to_string_lossy()
        .into_owned();
    let zmin_keep = first_pack_index(zmin_repo.path())
        .with_extension("pack")
        .file_name()
        .expect("zmin keep pack")
        .to_string_lossy()
        .into_owned();
    assert_eq!(zmin_keep, git_keep);

    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "b.txt", "two\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "two"]);
    }
    git(
        git_repo.path(),
        ["repack", "-adq", "--keep-pack", &git_keep],
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["repack", "-adq", "--keep-pack", &zmin_keep]
        ),
        ""
    );
    let git_packs = pack_file_names(git_repo.path());
    let zmin_packs = pack_file_names(zmin_repo.path());
    assert_eq!(
        zmin_packs.contains(&zmin_keep),
        git_packs.contains(&git_keep)
    );
    assert_eq!(
        zmin_packs
            .iter()
            .filter(|name| name.ends_with(".pack"))
            .count(),
        git_packs
            .iter()
            .filter(|name| name.ends_with(".pack"))
            .count()
    );
    assert_eq!(
        git(zmin_repo.path(), ["cat-file", "-p", "HEAD:b.txt"]),
        git(git_repo.path(), ["cat-file", "-p", "HEAD:b.txt"])
    );
}

#[test]
fn repack_keep_pack_ignores_non_pack_filename_forms_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        write_file(repo, "a.txt", "one\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "one"]);
        git(repo, ["repack", "-q"]);
    }
    let git_keep = first_pack_index(git_repo.path())
        .with_extension("pack")
        .file_name()
        .expect("git keep pack")
        .to_string_lossy()
        .into_owned();
    let zmin_keep = first_pack_index(zmin_repo.path())
        .with_extension("pack")
        .file_name()
        .expect("zmin keep pack")
        .to_string_lossy()
        .into_owned();
    assert_eq!(zmin_keep, git_keep);
    let keep_without_extension = git_keep.trim_end_matches(".pack").to_owned();

    for repo in [git_repo.path(), zmin_repo.path()] {
        write_file(repo, "b.txt", "two\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "two"]);
    }
    git(
        git_repo.path(),
        ["repack", "-adq", "--keep-pack", &keep_without_extension],
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["repack", "-adq", "--keep-pack", &keep_without_extension]
        ),
        ""
    );
    let git_packs = pack_file_names(git_repo.path());
    let zmin_packs = pack_file_names(zmin_repo.path());
    assert_eq!(
        zmin_packs.contains(&zmin_keep),
        git_packs.contains(&git_keep)
    );
    assert_eq!(
        zmin_packs
            .iter()
            .filter(|name| name.ends_with(".pack"))
            .count(),
        git_packs
            .iter()
            .filter(|name| name.ends_with(".pack"))
            .count()
    );
}

#[test]
fn repack_no_reuse_flags_write_stock_readable_pack() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    git(repo.path(), ["repack", "-adq"]);
    let head = git(repo.path(), ["rev-parse", "HEAD"]);

    assert_eq!(run_zmin(repo.path(), ["repack", "-adqfF"]), "");
    assert!(first_pack_index(repo.path()).is_file());
    assert_eq!(git_status(repo.path(), ["fsck", "--strict"]), 0);
    assert!(git(repo.path(), ["cat-file", "-p", &head]).contains("\n\ntwo"));
}

#[test]
fn repack_window_depth_writes_stock_readable_delta_pack() {
    let repo = git_init();
    let base_content = format!("{}\nbase\n", "shared line\n".repeat(2_000));
    let changed_content = format!("{}\nchanged\n", "shared line\n".repeat(2_000));
    let base = git_with_stdin(repo.path(), ["hash-object", "-w", "--stdin"], &base_content);
    let changed = git_with_stdin(
        repo.path(),
        ["hash-object", "-w", "--stdin"],
        &changed_content,
    );

    assert_eq!(
        run_zmin(repo.path(), ["repack", "--window=10", "--depth=10", "-q"]),
        ""
    );
    let idx = first_pack_index(repo.path());
    let verify = git(
        repo.path(),
        ["verify-pack", "-v", idx.to_str().expect("idx path")],
    );
    assert!(
        verify
            .lines()
            .any(|line| line.starts_with(&changed) && line.split_whitespace().count() >= 7),
        "expected changed blob to be stored as a delta:\n{verify}"
    );
    assert_eq!(
        command_stdout_bytes("git", repo.path(), &["cat-file", "-p", &base]),
        base_content.as_bytes()
    );
    assert_eq!(
        command_stdout_bytes("git", repo.path(), &["cat-file", "-p", &changed]),
        changed_content.as_bytes()
    );
}

#[test]
fn repack_local_keeps_alternate_objects_out_of_local_pack() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_clone = dir.path().join("git-shared");
    let zmin_clone = dir.path().join("zmin-shared");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    write_file(&source, "a.txt", "base\n");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);
    git(
        dir.path(),
        [
            "clone",
            "--shared",
            source.to_str().expect("source path"),
            git_clone.to_str().expect("git clone path"),
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            "--shared",
            source.to_str().expect("source path"),
            zmin_clone.to_str().expect("zmin clone path"),
        ],
    );
    for repo in [&git_clone, &zmin_clone] {
        configure_identity(repo);
        write_file(repo, "a.txt", "local\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "local"]);
    }

    git(&git_clone, ["repack", "-adlq"]);
    assert_eq!(run_zmin(&zmin_clone, ["repack", "-adlq"]), "");
    assert_eq!(
        packed_object_ids(&zmin_clone),
        packed_object_ids(&git_clone)
    );
    assert_eq!(
        git(&zmin_clone, ["cat-file", "-p", "HEAD:a.txt"]),
        git(&git_clone, ["cat-file", "-p", "HEAD:a.txt"])
    );
    assert_eq!(
        git(&zmin_clone, ["cat-file", "-p", "HEAD^:a.txt"]),
        git(&git_clone, ["cat-file", "-p", "HEAD^:a.txt"])
    );
}

#[test]
fn gc_packs_reachable_objects_and_keeps_head_readable() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    write_file(repo.path(), "a.txt", "two\n");
    write_file(repo.path(), "b.txt", "bee\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    let head = git(repo.path(), ["rev-parse", "HEAD"]);
    let objects = git(repo.path(), ["rev-list", "--objects", "HEAD"])
        .lines()
        .map(|line| {
            line.split_whitespace()
                .next()
                .expect("object id")
                .to_owned()
        })
        .collect::<Vec<_>>();

    assert_eq!(run_zmin(repo.path(), ["gc", "--prune=now", "--quiet"]), "");
    assert!(first_pack_index(repo.path()).is_file());
    for id in &objects {
        assert!(
            !loose_object_exists(repo.path(), id),
            "loose object should be packed and pruned after gc: {id}"
        );
        assert!(!git(repo.path(), ["cat-file", "-t", id]).is_empty());
    }
    assert!(git(repo.path(), ["cat-file", "-p", &head]).contains("\n\ntwo"));
}

#[test]
fn gc_aggressive_writes_stock_readable_delta_pack() {
    fn seed(repo: &std::path::Path) -> (String, String) {
        configure_identity(repo);
        write_file(
            repo,
            "delta.txt",
            &format!("{}\nbase\n", "shared line\n".repeat(2_000)),
        );
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "base"]);
        write_file(
            repo,
            "delta.txt",
            &format!("{}\nchanged\n", "shared line\n".repeat(2_000)),
        );
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "changed"]);
        let base = git(repo, ["rev-parse", "HEAD~1:delta.txt"]);
        let changed = git(repo, ["rev-parse", "HEAD:delta.txt"]);
        (base, changed)
    }

    let git_repo = git_init();
    let zmin_repo = git_init();
    let (git_base, git_changed) = seed(git_repo.path());
    let (zmin_base, zmin_changed) = seed(zmin_repo.path());
    assert_eq!(zmin_base, git_base);
    assert_eq!(zmin_changed, git_changed);

    assert_eq!(git(git_repo.path(), ["gc", "--aggressive", "--quiet"]), "");
    assert_eq!(
        run_zmin(zmin_repo.path(), ["gc", "--aggressive", "--quiet"]),
        ""
    );

    let git_verify = git(
        git_repo.path(),
        [
            "verify-pack",
            "-v",
            first_pack_index(git_repo.path())
                .to_str()
                .expect("idx path"),
        ],
    );
    let zmin_verify = git(
        zmin_repo.path(),
        [
            "verify-pack",
            "-v",
            first_pack_index(zmin_repo.path())
                .to_str()
                .expect("idx path"),
        ],
    );
    assert!(
        verify_pack_delta_base(&git_verify, &git_base).is_some()
            || verify_pack_delta_base(&git_verify, &git_changed).is_some(),
        "expected stock git to write a blob delta:\n{git_verify}"
    );
    assert_eq!(
        verify_pack_delta_base(&zmin_verify, &zmin_base),
        verify_pack_delta_base(&git_verify, &git_base),
        "expected base blob delta linkage to match stock git\nstock:\n{git_verify}\nzmin:\n{zmin_verify}"
    );
    assert_eq!(
        verify_pack_delta_base(&zmin_verify, &zmin_changed),
        verify_pack_delta_base(&git_verify, &git_changed),
        "expected changed blob delta linkage to match stock git\nstock:\n{git_verify}\nzmin:\n{zmin_verify}"
    );
    assert_eq!(git_status(zmin_repo.path(), ["fsck", "--strict"]), 0);
    assert!(git(zmin_repo.path(), ["cat-file", "-p", "HEAD"]).contains("\n\nchanged"));
}

#[test]
fn maintenance_run_gc_reuses_repository_gc() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    let head = git(repo.path(), ["rev-parse", "HEAD"]);

    assert_eq!(
        run_zmin(repo.path(), ["maintenance", "run", "--task=gc", "--quiet"]),
        ""
    );
    assert!(first_pack_index(repo.path()).is_file());
    assert!(git(repo.path(), ["cat-file", "-p", &head]).contains("\n\none"));
}

#[test]
fn maintenance_run_local_tasks_create_stock_readable_metadata() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    git(repo.path(), ["branch", "feature"]);
    git_with_env(repo.path(), ["tag", "-a", "v1", "-m", "version"]);

    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "maintenance",
                "run",
                "--task=commit-graph",
                "--task=pack-refs",
                "--task=loose-objects",
            ],
        ),
        ""
    );
    assert!(repo.path().join(".git/objects/info/commit-graph").is_file());
    assert_eq!(git_status(repo.path(), ["commit-graph", "verify"]), 0);
    assert!(repo.path().join(".git/packed-refs").is_file());
    assert_eq!(
        git(repo.path(), ["rev-parse", "refs/heads/feature"]).len(),
        40
    );
    assert_eq!(git(repo.path(), ["rev-parse", "refs/tags/v1"]).len(), 40);
}

#[test]
fn maintenance_register_and_unregister_match_stock_git_config_effects() {
    let git_home = TempDir::new().expect("git home");
    let zmin_home = TempDir::new().expect("zmin home");
    let git_repo = git_init();
    let zmin_repo = git_init();

    command_output_with_env(
        "git",
        git_repo.path(),
        &["maintenance", "register"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["maintenance", "register"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "zmin",
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["config", "--local", "--get", "maintenance.auto"]
        ),
        git(
            git_repo.path(),
            ["config", "--local", "--get", "maintenance.auto"]
        )
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            ["config", "--local", "--get", "maintenance.strategy"],
        ),
        git(
            git_repo.path(),
            ["config", "--local", "--get", "maintenance.strategy"],
        )
    );
    let zmin_registered = command_output_with_env(
        "git",
        zmin_repo.path(),
        &["config", "--global", "--get-all", "maintenance.repo"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "git",
    )
    .1;
    assert_eq!(
        zmin_registered,
        git_path_output_string(
            fs::canonicalize(zmin_repo.path())
                .expect("canonical zmin repo")
                .display()
                .to_string()
        )
    );

    command_output_with_env(
        "git",
        git_repo.path(),
        &["maintenance", "unregister"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["maintenance", "unregister"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "zmin",
    );
    assert_eq!(
        fs::read_to_string(zmin_home.path().join(".gitconfig")).expect("zmin global config"),
        fs::read_to_string(git_home.path().join(".gitconfig")).expect("git global config")
    );
}

#[test]
fn maintenance_register_is_idempotent_like_stock_git() {
    let git_home = TempDir::new().expect("git home");
    let zmin_home = TempDir::new().expect("zmin home");
    let git_repo = git_init();
    let zmin_repo = git_init();
    for _ in 0..2 {
        command_output_with_env(
            "git",
            git_repo.path(),
            &["maintenance", "register"],
            &[("HOME", git_home.path().to_str().expect("git home path"))],
            "git",
        );
        command_output_with_env(
            zmin_bin(),
            zmin_repo.path(),
            &["maintenance", "register"],
            &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
            "zmin",
        );
    }

    let zmin_registered = command_output_with_env(
        "git",
        zmin_repo.path(),
        &["config", "--global", "--get-all", "maintenance.repo"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "git",
    )
    .1;
    let git_registered = command_output_with_env(
        "git",
        git_repo.path(),
        &["config", "--global", "--get-all", "maintenance.repo"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    )
    .1;
    assert_eq!(
        zmin_registered.lines().count(),
        git_registered.lines().count()
    );
    assert_eq!(zmin_registered.lines().count(), 1);
}

#[test]
fn maintenance_unregister_missing_repo_failure_matches_stock_git() {
    let git_home = TempDir::new().expect("git home");
    let zmin_home = TempDir::new().expect("zmin home");
    let repo = git_init();

    assert_eq!(
        command_failure_output_with_env(
            zmin_bin(),
            repo.path(),
            &["maintenance", "unregister"],
            &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
            "zmin",
        ),
        command_failure_output_with_env(
            "git",
            repo.path(),
            &["maintenance", "unregister"],
            &[("HOME", git_home.path().to_str().expect("git home path"))],
            "git",
        )
    );
}

#[test]
fn maintenance_register_and_unregister_with_config_file_match_stock_git() {
    let git_home = TempDir::new().expect("git home");
    let zmin_home = TempDir::new().expect("zmin home");
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_config = git_home.path().join("custom.gitconfig");
    let zmin_config = zmin_home.path().join("custom.gitconfig");

    command_output_with_env(
        "git",
        git_repo.path(),
        &[
            "maintenance",
            "register",
            "--config-file",
            git_config.to_str().expect("git config path"),
        ],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "maintenance",
            "register",
            "--config-file",
            zmin_config.to_str().expect("zmin config path"),
        ],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "zmin",
    );

    assert_eq!(
        normalize_repo_path(
            fs::read_to_string(&zmin_config).expect("read zmin config"),
            zmin_repo.path(),
        ),
        normalize_repo_path(
            fs::read_to_string(&git_config).expect("read git config"),
            git_repo.path(),
        )
    );

    command_output_with_env(
        "git",
        git_repo.path(),
        &[
            "maintenance",
            "unregister",
            "--config-file",
            git_config.to_str().expect("git config path"),
        ],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &[
            "maintenance",
            "unregister",
            "--config-file",
            zmin_config.to_str().expect("zmin config path"),
        ],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "zmin",
    );

    assert_eq!(
        normalize_repo_path(
            fs::read_to_string(&zmin_config).expect("read zmin config"),
            zmin_repo.path(),
        ),
        normalize_repo_path(
            fs::read_to_string(&git_config).expect("read git config"),
            git_repo.path(),
        )
    );
}

#[test]
fn maintenance_unregister_force_missing_repo_matches_stock_git() {
    let git_home = TempDir::new().expect("git home");
    let zmin_home = TempDir::new().expect("zmin home");
    let repo = git_init();

    assert_eq!(
        command_output_with_env(
            zmin_bin(),
            repo.path(),
            &["maintenance", "unregister", "--force"],
            &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
            "zmin",
        ),
        command_output_with_env(
            "git",
            repo.path(),
            &["maintenance", "unregister", "--force"],
            &[("HOME", git_home.path().to_str().expect("git home path"))],
            "git",
        )
    );
}

#[cfg(target_os = "macos")]
fn command_any_output_with_env(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
) -> (i32, String, String) {
    let output = std::process::Command::new(common::test_command_program(command))
        .args(args)
        .envs(envs.iter().copied())
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run {label}: {err}"));
    (
        output.status.code().expect("process exit code"),
        String::from_utf8(output.stdout)
            .expect("stdout utf8")
            .trim_end_matches('\n')
            .to_owned(),
        String::from_utf8(output.stderr)
            .expect("stderr utf8")
            .trim_end_matches('\n')
            .to_owned(),
    )
}

#[cfg(target_os = "macos")]
fn stock_git_scheduler_runtime_blocked(output: &(i32, String, String)) -> bool {
    output.2.contains("Operation not permitted")
        || output.2.contains("failed to bootstrap service")
        || output.2.contains("Bootstrap failed")
}

#[cfg(target_os = "macos")]
#[test]
fn maintenance_unavailable_schedulers_match_stock_git_on_macos() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for scheduler in ["crontab", "systemd-timer", "schtasks"] {
        let git_output = git_failure_output(
            git_repo.path(),
            &["maintenance", "start", &format!("--scheduler={scheduler}")],
        );
        if stock_git_scheduler_runtime_blocked(&git_output) {
            eprintln!(
                "skipping scheduler {scheduler} parity; stock git scheduler runtime is blocked in this environment: {}",
                git_output.2
            );
            continue;
        }
        assert_eq!(
            run_zmin_failure_output(
                zmin_repo.path(),
                &["maintenance", "start", &format!("--scheduler={scheduler}")]
            ),
            git_output,
            "scheduler {scheduler} failure should match stock git"
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn maintenance_start_and_stop_auto_manage_launch_agents_like_stock_git() {
    let git_home = TempDir::new().expect("git home");
    let zmin_home = TempDir::new().expect("zmin home");
    let git_repo = git_init();
    let zmin_repo = git_init();

    let git_start = command_any_output_with_env(
        "git",
        git_repo.path(),
        &["maintenance", "start", "--scheduler=auto"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    if stock_git_scheduler_runtime_blocked(&git_start) {
        eprintln!(
            "skipping launchctl parity; stock git scheduler runtime is blocked in this environment: {}",
            git_start.2
        );
        return;
    }

    let git_registered = command_output_with_env(
        "git",
        git_repo.path(),
        &["config", "--global", "--get-all", "maintenance.repo"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    let git_launch_agent_presence: Vec<_> = [
        "org.git-scm.git.hourly.plist",
        "org.git-scm.git.daily.plist",
        "org.git-scm.git.weekly.plist",
    ]
    .into_iter()
    .map(|name| {
        (
            name,
            git_home
                .path()
                .join("Library/LaunchAgents")
                .join(name)
                .exists(),
        )
    })
    .collect();
    let git_stop = command_output_with_env(
        "git",
        git_repo.path(),
        &["maintenance", "stop"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    let git_launch_agent_cleanup: Vec<_> = [
        "org.git-scm.git.hourly.plist",
        "org.git-scm.git.daily.plist",
        "org.git-scm.git.weekly.plist",
    ]
    .into_iter()
    .map(|name| {
        (
            name,
            git_home
                .path()
                .join("Library/LaunchAgents")
                .join(name)
                .exists(),
        )
    })
    .collect();

    let zmin_start = command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["maintenance", "start", "--scheduler=auto"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "zmin",
    );
    assert_eq!(
        zmin_start.0, 0,
        "zmin launchctl start should succeed when stock git launchctl runtime is available: {zmin_start:?}"
    );
    assert_eq!(zmin_start.0, git_start.0);
    assert_eq!(
        normalize_repo_path(zmin_start.1, zmin_repo.path()),
        normalize_repo_path(git_start.1, git_repo.path())
    );
    assert_eq!(zmin_start.2, git_start.2);

    let zmin_registered = command_output_with_env(
        "git",
        zmin_repo.path(),
        &["config", "--global", "--get-all", "maintenance.repo"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "git",
    );
    assert_eq!(zmin_registered.0, git_registered.0);
    assert_eq!(
        normalize_repo_path(zmin_registered.1, zmin_repo.path()),
        normalize_repo_path(git_registered.1, git_repo.path())
    );
    assert_eq!(zmin_registered.2, git_registered.2);

    for (name, git_exists) in git_launch_agent_presence {
        assert_eq!(
            zmin_home
                .path()
                .join("Library/LaunchAgents")
                .join(name)
                .exists(),
            git_exists,
            "launch agent presence should match for {name}"
        );
    }

    let zmin_stop = command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["maintenance", "stop"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "zmin",
    );
    assert_eq!(zmin_stop.0, git_stop.0);

    for (name, git_exists) in git_launch_agent_cleanup {
        assert_eq!(
            zmin_home
                .path()
                .join("Library/LaunchAgents")
                .join(name)
                .exists(),
            git_exists,
            "launch agent cleanup should match for {name}"
        );
    }
}

#[cfg(windows)]
fn schtasks_task_exists(name: &str) -> bool {
    std::process::Command::new("schtasks")
        .args(["/query", "/tn", name])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(windows)]
fn schtasks_delete_task_if_present(name: &str) {
    let _ = std::process::Command::new("schtasks")
        .args(["/delete", "/tn", name, "/f"])
        .output();
}

#[cfg(windows)]
fn schtasks_task_xml(name: &str) -> String {
    let output = std::process::Command::new("schtasks")
        .args(["/query", "/tn", name, "/xml"])
        .output()
        .expect("query schtasks xml");
    assert!(
        output.status.success(),
        "schtasks /query /xml failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("schtasks xml utf8")
}

#[cfg(windows)]
fn assert_schtasks_task_xml_matches_expected_shape(name: &str, schedule: &str, binary_name: &str) {
    let xml = schtasks_task_xml(name);
    assert!(
        xml.contains(binary_name),
        "expected task {name} XML command to contain {binary_name}:\n{xml}"
    );
    assert!(
        xml.contains(&format!(
            "for-each-repo --keep-going --config=maintenance.repo maintenance run --schedule={schedule}"
        )),
        "expected task {name} XML arguments for schedule {schedule}:\n{xml}"
    );
    match schedule {
        "hourly" => assert!(
            xml.contains("<Interval>PT1H</Interval>") && xml.contains("<Duration>PT23H</Duration>"),
            "expected hourly repetition in task {name}:\n{xml}"
        ),
        "daily" => assert!(
            xml.contains("<Monday") && xml.contains("<Saturday") && !xml.contains("<Sunday"),
            "expected daily weekday schedule in task {name}:\n{xml}"
        ),
        "weekly" => assert!(
            xml.contains("<Sunday"),
            "expected weekly Sunday schedule in task {name}:\n{xml}"
        ),
        other => panic!("unsupported schedule {other}"),
    }
}

#[cfg(target_os = "linux")]
fn crontab_contents() -> String {
    let output = std::process::Command::new("crontab")
        .arg("-l")
        .output()
        .expect("run crontab -l");
    if output.status.success() {
        return String::from_utf8(output.stdout)
            .expect("crontab stdout utf8")
            .trim_end_matches('\n')
            .to_owned();
    }
    let stderr = String::from_utf8(output.stderr).expect("crontab stderr utf8");
    if stderr.contains("no crontab for") {
        return String::new();
    }
    panic!("crontab -l failed: {stderr}");
}

#[cfg(target_os = "linux")]
fn clear_crontab() {
    let _ = std::process::Command::new("crontab").arg("-r").output();
}

#[cfg(target_os = "linux")]
fn normalize_crontab(text: String, repo: &std::path::Path) -> String {
    let normalized_repo = normalize_repo_path(text, repo);
    normalized_repo
        .lines()
        .map(|line| {
            if let Some(index) = line.find(" maintenance run --schedule=") {
                let mut fields = line[..index].split_whitespace().take(5).collect::<Vec<_>>();
                if !fields.is_empty() {
                    fields[0] = "<minute>";
                }
                let schedule = fields.join(" ");
                let suffix = &line[index..];
                format!("{schedule}{suffix}")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(windows)]
#[test]
fn maintenance_start_and_stop_schtasks_match_stock_git_when_enabled() {
    if std::env::var_os("ZMIN_ENABLE_WINDOWS_MAINTENANCE_RUNTIME_TESTS").is_none() {
        eprintln!(
            "skipping schtasks runtime test; set ZMIN_ENABLE_WINDOWS_MAINTENANCE_RUNTIME_TESTS=1 to enable"
        );
        return;
    }

    let tasks = [
        ("Git Maintenance (hourly)", "hourly"),
        ("Git Maintenance (daily)", "daily"),
        ("Git Maintenance (weekly)", "weekly"),
    ];
    for (task, _) in tasks {
        schtasks_delete_task_if_present(task);
    }

    let git_home = TempDir::new().expect("git home");
    let zmin_home = TempDir::new().expect("zmin home");
    let git_repo = git_init();
    let zmin_repo = git_init();

    let zmin_start = command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["maintenance", "start", "--scheduler=schtasks"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "zmin",
    );
    for (task, schedule) in tasks {
        assert!(
            schtasks_task_exists(task),
            "expected scheduled task {task} after zmin start"
        );
        assert_schtasks_task_xml_matches_expected_shape(task, schedule, "zmin");
    }

    let zmin_registered = command_output_with_env(
        "git",
        zmin_repo.path(),
        &["config", "--global", "--get-all", "maintenance.repo"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "git",
    );

    let zmin_stop = command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["maintenance", "stop"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "zmin",
    );
    for (task, _) in tasks {
        assert!(
            !schtasks_task_exists(task),
            "expected scheduled task {task} to be removed after zmin stop"
        );
    }

    let git_start = command_output_with_env(
        "git",
        git_repo.path(),
        &["maintenance", "start", "--scheduler=schtasks"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    for (task, schedule) in tasks {
        assert!(
            schtasks_task_exists(task),
            "expected scheduled task {task} after git start"
        );
        assert_schtasks_task_xml_matches_expected_shape(task, schedule, "git");
    }

    let git_registered = command_output_with_env(
        "git",
        git_repo.path(),
        &["config", "--global", "--get-all", "maintenance.repo"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );

    let git_stop = command_output_with_env(
        "git",
        git_repo.path(),
        &["maintenance", "stop"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    for (task, _) in tasks {
        assert!(
            !schtasks_task_exists(task),
            "expected scheduled task {task} to be removed after git stop"
        );
    }

    assert_eq!(zmin_start.0, git_start.0);
    assert_eq!(
        normalize_repo_path(zmin_start.1, zmin_repo.path()),
        normalize_repo_path(git_start.1, git_repo.path())
    );
    assert_eq!(zmin_start.2, git_start.2);

    assert_eq!(zmin_stop.0, git_stop.0);
    assert_eq!(zmin_stop.1, git_stop.1);
    assert_eq!(zmin_stop.2, git_stop.2);

    assert_eq!(zmin_registered.0, git_registered.0);
    assert_eq!(
        normalize_repo_path(zmin_registered.1, zmin_repo.path()),
        normalize_repo_path(git_registered.1, git_repo.path())
    );
    assert_eq!(zmin_registered.2, git_registered.2);
}

#[cfg(target_os = "linux")]
#[test]
fn maintenance_start_and_stop_crontab_match_stock_git_when_enabled() {
    if std::env::var_os("ZMIN_ENABLE_LINUX_MAINTENANCE_RUNTIME_TESTS").is_none() {
        eprintln!(
            "skipping linux maintenance runtime test; set ZMIN_ENABLE_LINUX_MAINTENANCE_RUNTIME_TESTS=1 to enable"
        );
        return;
    }

    clear_crontab();

    let git_home = TempDir::new().expect("git home");
    let zmin_home = TempDir::new().expect("zmin home");
    let git_repo = git_init();
    let zmin_repo = git_init();

    let zmin_start = command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["maintenance", "start", "--scheduler=crontab"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "zmin",
    );
    let zmin_crontab = crontab_contents();
    let zmin_registered = command_output_with_env(
        "git",
        zmin_repo.path(),
        &["config", "--global", "--get-all", "maintenance.repo"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "git",
    );
    let zmin_stop = command_output_with_env(
        zmin_bin(),
        zmin_repo.path(),
        &["maintenance", "stop"],
        &[("HOME", zmin_home.path().to_str().expect("zmin home path"))],
        "zmin",
    );
    let zmin_crontab_after_stop = crontab_contents();

    clear_crontab();

    let git_start = command_output_with_env(
        "git",
        git_repo.path(),
        &["maintenance", "start", "--scheduler=crontab"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    let git_crontab = crontab_contents();
    let git_registered = command_output_with_env(
        "git",
        git_repo.path(),
        &["config", "--global", "--get-all", "maintenance.repo"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    let git_stop = command_output_with_env(
        "git",
        git_repo.path(),
        &["maintenance", "stop"],
        &[("HOME", git_home.path().to_str().expect("git home path"))],
        "git",
    );
    let git_crontab_after_stop = crontab_contents();

    clear_crontab();

    assert_eq!(zmin_start.0, git_start.0);
    assert_eq!(
        normalize_repo_path(zmin_start.1, zmin_repo.path()),
        normalize_repo_path(git_start.1, git_repo.path())
    );
    assert_eq!(zmin_start.2, git_start.2);

    assert_eq!(
        normalize_crontab(zmin_crontab, zmin_repo.path()),
        normalize_crontab(git_crontab, git_repo.path())
    );

    assert_eq!(zmin_stop.0, git_stop.0);
    assert_eq!(zmin_stop.1, git_stop.1);
    assert_eq!(zmin_stop.2, git_stop.2);
    assert_eq!(zmin_crontab_after_stop, git_crontab_after_stop);

    assert_eq!(zmin_registered.0, git_registered.0);
    assert_eq!(
        normalize_repo_path(zmin_registered.1, zmin_repo.path()),
        normalize_repo_path(git_registered.1, git_repo.path())
    );
    assert_eq!(zmin_registered.2, git_registered.2);
}

#[test]
fn maintenance_incremental_repack_writes_stock_verifiable_multi_pack_index() {
    let repo = git_init();
    configure_identity(repo.path());
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    git(repo.path(), ["repack", "-adq"]);

    assert_eq!(
        run_zmin(
            repo.path(),
            ["maintenance", "run", "--task=incremental-repack"]
        ),
        ""
    );
    assert!(
        repo.path()
            .join(".git/objects/pack/multi-pack-index")
            .is_file()
    );
    assert_eq!(run_zmin(repo.path(), ["multi-pack-index", "verify"]), "");
    assert_eq!(git_status(repo.path(), ["multi-pack-index", "verify"]), 0);
}

#[test]
fn maintenance_incremental_repack_empty_repo_failure_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &["maintenance", "run", "--task=incremental-repack"]
        ),
        git_failure_output(
            git_repo.path(),
            &["maintenance", "run", "--task=incremental-repack"]
        )
    );
}

#[test]
fn maintenance_run_schedule_matches_stock_git_strategy_selection() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["maintenance", "run", "--schedule=hourly"]
        ),
        git(git_repo.path(), ["maintenance", "run", "--schedule=hourly"])
    );

    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        git(repo, ["config", "maintenance.strategy", "incremental"]);
        write_file(repo, "a.txt", "one\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "one"]);
        git(repo, ["repack", "-adq"]);
    }
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["maintenance", "run", "--schedule=daily", "--quiet"]
        ),
        git(
            git_repo.path(),
            ["maintenance", "run", "--schedule=daily", "--quiet"]
        )
    );
    assert_eq!(git_status(zmin_repo.path(), ["commit-graph", "verify"]), 0);
    assert_eq!(
        git_status(zmin_repo.path(), ["multi-pack-index", "verify"]),
        0
    );
    assert_eq!(
        zmin_repo
            .path()
            .join(".git/objects/pack/multi-pack-index")
            .is_file(),
        git_repo
            .path()
            .join(".git/objects/pack/multi-pack-index")
            .is_file()
    );
}

#[test]
fn maintenance_run_daily_prunes_packed_loose_duplicates_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        git(repo, ["config", "maintenance.strategy", "incremental"]);
        write_file(repo, "a.txt", "one\n");
        git(repo, ["add", "-A"]);
        git_with_env(repo, ["commit", "-m", "one"]);
    }
    let git_head = duplicate_packed_head_as_loose(git_repo.path());
    let zmin_head = duplicate_packed_head_as_loose(zmin_repo.path());
    assert_eq!(zmin_head, git_head);

    git(
        git_repo.path(),
        ["maintenance", "run", "--schedule=daily", "--quiet"],
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["maintenance", "run", "--schedule=daily", "--quiet"]
        ),
        ""
    );
    assert_eq!(
        loose_object_exists(zmin_repo.path(), &zmin_head),
        loose_object_exists(git_repo.path(), &git_head)
    );
}

#[test]
fn maintenance_run_weekly_packs_refs_like_stock_git() {
    let git_repo = pack_refs_fixture_repo();
    let zmin_repo = pack_refs_fixture_repo();
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "maintenance.strategy", "incremental"]);
        git(repo, ["repack", "-adq"]);
    }

    git(
        git_repo.path(),
        ["maintenance", "run", "--schedule=weekly", "--quiet"],
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["maintenance", "run", "--schedule=weekly", "--quiet"]
        ),
        ""
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join(".git/packed-refs"))
            .expect("read zmin packed refs"),
        fs::read_to_string(git_repo.path().join(".git/packed-refs")).expect("read git packed refs")
    );
    assert_eq!(
        ref_file_list(zmin_repo.path()),
        ref_file_list(git_repo.path())
    );
}

#[test]
fn maintenance_run_invalid_schedule_failure_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin_failure_output(
            zmin_repo.path(),
            &["maintenance", "run", "--schedule=invalid"]
        ),
        git_failure_output(
            git_repo.path(),
            &["maintenance", "run", "--schedule=invalid"]
        )
    );
}

#[test]
fn maintenance_run_schedule_task_and_auto_failures_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for args in [
        ["maintenance", "run", "--schedule=invalid", "--task=gc"].as_slice(),
        ["maintenance", "run", "--auto", "--schedule=daily"].as_slice(),
        ["maintenance", "run", "--task=missing", "--schedule=daily"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), args),
            git_failure_output(git_repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn maintenance_prefetch_noops_without_remotes_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    assert_eq!(
        run_zmin(zmin_repo.path(), ["maintenance", "run", "--task=prefetch"]),
        git(git_repo.path(), ["maintenance", "run", "--task=prefetch"])
    );
    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            [
                "maintenance",
                "run",
                "--task=gc",
                "--task=prefetch",
                "--quiet"
            ]
        ),
        git(
            git_repo.path(),
            [
                "maintenance",
                "run",
                "--task=gc",
                "--task=prefetch",
                "--quiet"
            ]
        )
    );
}

#[test]
fn maintenance_prefetch_local_remote_writes_prefetch_refs_like_stock_git() {
    let source = git_init();
    configure_identity(source.path());
    git(source.path(), ["checkout", "-b", "main"]);
    write_file(source.path(), "main.txt", "main\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "main"]);
    git(source.path(), ["checkout", "-b", "feature"]);
    write_file(source.path(), "feature.txt", "feature\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "feature"]);

    let git_repo = git_init();
    let zmin_repo = git_init();
    git(
        git_repo.path(),
        ["remote", "add", "origin", source.path().to_str().unwrap()],
    );
    git(
        zmin_repo.path(),
        ["remote", "add", "origin", source.path().to_str().unwrap()],
    );

    assert_eq!(
        run_zmin(zmin_repo.path(), ["maintenance", "run", "--task=prefetch"]),
        git(git_repo.path(), ["maintenance", "run", "--task=prefetch"])
    );
    assert_eq!(
        git(
            zmin_repo.path(),
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/prefetch",
            ],
        ),
        git(
            git_repo.path(),
            [
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/prefetch",
            ],
        )
    );
    assert_eq!(git(zmin_repo.path(), ["fsck", "--strict"]), "");
}

#[test]
fn maintenance_prefetch_unsupported_remote_helper_failure_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(
            repo,
            ["remote", "add", "origin", "zminproto://example/repo"],
        );
    }

    assert_eq!(
        run_zmin_failure_output(zmin_repo.path(), &["maintenance", "run", "--task=prefetch"]),
        git_failure_output(git_repo.path(), &["maintenance", "run", "--task=prefetch"])
    );
}

#[test]
fn prune_matches_stock_git_for_unreachable_loose_objects() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let (git_pruned, git_kept, git_staged) = prune_fixture(git_repo.path());
    let (zmin_pruned, zmin_kept, zmin_staged) = prune_fixture(zmin_repo.path());
    assert_eq!(zmin_pruned, git_pruned);
    assert_eq!(zmin_kept, git_kept);
    assert_eq!(zmin_staged, git_staged);

    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            ["prune", "-n", "--expire=now", &zmin_kept]
        ),
        git(git_repo.path(), ["prune", "-n", "--expire=now", &git_kept])
    );
    assert!(loose_object_exists(zmin_repo.path(), &zmin_pruned));

    git(git_repo.path(), ["prune", "--expire=now", &git_kept]);
    run_zmin(zmin_repo.path(), ["prune", "--expire=now", &zmin_kept]);
    assert_eq!(
        loose_object_exists(zmin_repo.path(), &zmin_pruned),
        loose_object_exists(git_repo.path(), &git_pruned)
    );
    assert_eq!(
        loose_object_exists(zmin_repo.path(), &zmin_kept),
        loose_object_exists(git_repo.path(), &git_kept)
    );
    assert_eq!(
        loose_object_exists(zmin_repo.path(), &zmin_staged),
        loose_object_exists(git_repo.path(), &git_staged)
    );
}

#[test]
fn prune_without_expire_matches_stock_git_for_unreachable_loose_objects() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_pruned = git_with_stdin(
        git_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "prune me\n",
    );
    let zmin_pruned = git_with_stdin(
        zmin_repo.path(),
        ["hash-object", "-w", "--stdin"],
        "prune me\n",
    );
    assert_eq!(zmin_pruned, git_pruned);

    git(git_repo.path(), ["prune"]);
    run_zmin(zmin_repo.path(), ["prune"]);
    assert_eq!(
        loose_object_exists(zmin_repo.path(), &zmin_pruned),
        loose_object_exists(git_repo.path(), &git_pruned)
    );
}

#[test]
fn prune_preserves_objects_reachable_from_reflogs() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let git_reflog_commit = reflog_prune_fixture(git_repo.path());
    let zmin_reflog_commit = reflog_prune_fixture(zmin_repo.path());
    assert_eq!(zmin_reflog_commit, git_reflog_commit);

    git(git_repo.path(), ["prune", "--expire=now"]);
    run_zmin(zmin_repo.path(), ["prune", "--expire=now"]);
    assert_eq!(
        loose_object_exists(zmin_repo.path(), &zmin_reflog_commit),
        loose_object_exists(git_repo.path(), &git_reflog_commit)
    );
}

#[test]
fn prune_matches_stock_git_for_negated_options() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let (git_pruned, git_kept, _) = prune_fixture(git_repo.path());
    let (zmin_pruned, zmin_kept, _) = prune_fixture(zmin_repo.path());
    assert_eq!(zmin_pruned, git_pruned);
    assert_eq!(zmin_kept, git_kept);

    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            [
                "prune",
                "--no-exclude-promisor-objects",
                "--expire=never",
                &zmin_kept,
            ],
        ),
        git(
            git_repo.path(),
            [
                "prune",
                "--no-exclude-promisor-objects",
                "--expire=never",
                &git_kept,
            ],
        )
    );
    assert!(loose_object_exists(zmin_repo.path(), &zmin_pruned));

    assert_eq!(
        run_zmin(
            zmin_repo.path(),
            [
                "prune",
                "-n",
                "--no-dry-run",
                "-v",
                "--no-verbose",
                "--no-progress",
                "--expire=now",
                &zmin_kept,
            ],
        ),
        git(
            git_repo.path(),
            [
                "prune",
                "-n",
                "--no-dry-run",
                "-v",
                "--no-verbose",
                "--no-progress",
                "--expire=now",
                &git_kept,
            ],
        )
    );
    assert_eq!(
        loose_object_exists(zmin_repo.path(), &zmin_pruned),
        loose_object_exists(git_repo.path(), &git_pruned)
    );
}
