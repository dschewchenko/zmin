mod common;

use std::{collections::BTreeSet, fs};

use tempfile::TempDir;
use zmin_git_core::{GitHashAlgorithm, GitObjectHash};

use common::{
    clone_repo_fixture, command_any_output, command_any_output_with_stdin,
    command_any_output_with_stdin_bytes, command_failure_output_with_env, command_output_with_env,
    command_stdout_bytes, configure_identity, git, git_args, git_failure_output, git_init,
    git_status, git_with_env, git_with_stdin, git_with_stdin_bytes, run_zmin, run_zmin_args,
    run_zmin_failure_output, run_zmin_status, run_zmin_with_env, run_zmin_with_stdin,
    run_zmin_with_stdin_bytes, zmin_bin,
};

fn first_pack_index(repo: &std::path::Path) -> std::path::PathBuf {
    let mut paths = fs::read_dir(repo.join(".git/objects/pack"))
        .expect("read pack dir")
        .map(|entry| entry.expect("pack entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("idx"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.into_iter().next().expect("pack index")
}

fn pack_contains_object(repo: &std::path::Path, object_id: &str) -> bool {
    git(
        repo,
        [
            "verify-pack",
            "--verbose",
            first_pack_index(repo).to_str().expect("pack index path"),
        ],
    )
    .lines()
    .any(|line| line.split_whitespace().next() == Some(object_id))
}

fn rewrite_pack_index_version(path: &std::path::Path, version: u32) -> Vec<u8> {
    let mut bytes = fs::read(path).expect("read pack index");
    bytes[4..8].copy_from_slice(&version.to_be_bytes());
    let checksum_start = bytes.len() - GitHashAlgorithm::Sha1.digest_len();
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&bytes[..checksum_start]);
    let checksum = hasher.finalize();
    bytes[checksum_start..].copy_from_slice(checksum.as_bytes());
    bytes
}

fn two_commit_repo() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write first");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    fs::write(repo.path().join("a.txt"), b"two\n").expect("write second");
    fs::write(repo.path().join("b.txt"), b"two\n").expect("write added");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    repo
}

fn loose_object_path(repo: &std::path::Path, hex: &str) -> std::path::PathBuf {
    repo.join(".git/objects").join(&hex[..2]).join(&hex[2..])
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|chunk| {
            let value = std::str::from_utf8(chunk).expect("hex chunk utf8");
            u8::from_str_radix(value, 16).expect("hex byte")
        })
        .collect()
}

fn init_promisor_work_repo(remote: &std::path::Path, branch: &str) -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(
        repo.path(),
        [
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path utf8"),
        ],
    );
    git(repo.path(), ["fetch", "origin"]);
    git(
        repo.path(),
        ["checkout", "-B", branch, &format!("origin/{branch}")],
    );
    git(repo.path(), ["config", "remote.origin.promisor", "true"]);
    repo
}

#[test]
fn hash_object_and_cat_file_match_stock_git() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");

    let git_id = git(repo.path(), ["hash-object", "-w", "a.txt"]);
    let zmin_id = run_zmin(repo.path(), ["hash-object", "-w", "a.txt"]);
    assert_eq!(zmin_id, git_id);
    assert_eq!(
        run_zmin(repo.path(), ["hash-object", "/dev/null"]),
        git(repo.path(), ["hash-object", "/dev/null"])
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["hash-object", "--stdin", "a.txt"], "stdin\n"),
        git_with_stdin(repo.path(), ["hash-object", "--stdin", "a.txt"], "stdin\n")
    );
    let large_stdin = vec![b'x'; 300 * 1024];
    let git_large_id =
        git_with_stdin_bytes(repo.path(), ["hash-object", "-w", "--stdin"], &large_stdin);
    let zmin_large_id =
        run_zmin_with_stdin_bytes(repo.path(), ["hash-object", "-w", "--stdin"], &large_stdin);
    assert_eq!(zmin_large_id, git_large_id);
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-s", &zmin_large_id]),
        git(repo.path(), ["cat-file", "-s", &git_large_id])
    );

    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-t", &git_id]),
        git(repo.path(), ["cat-file", "-t", &git_id])
    );
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-s", &git_id]),
        git(repo.path(), ["cat-file", "-s", &git_id])
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            ["cat-file", "--batch-all-objects", "--batch-check"]
        ),
        git(
            repo.path(),
            ["cat-file", "--batch-all-objects", "--batch-check"]
        )
    );
    let zmin_unordered = run_zmin(
        repo.path(),
        [
            "cat-file",
            "--batch-all-objects",
            "--batch-check",
            "--unordered",
        ],
    );
    let git_unordered = git(
        repo.path(),
        [
            "cat-file",
            "--batch-all-objects",
            "--batch-check",
            "--unordered",
        ],
    );
    assert_eq!(
        zmin_unordered.lines().collect::<BTreeSet<_>>(),
        git_unordered.lines().collect::<BTreeSet<_>>()
    );
    assert_eq!(
        run_zmin(
            repo.path(),
            [
                "cat-file",
                "--batch-all-objects",
                "--batch-check",
                "--no-unordered",
            ],
        ),
        git(
            repo.path(),
            [
                "cat-file",
                "--batch-all-objects",
                "--batch-check",
                "--no-unordered",
            ],
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check=%(objectname)"],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check=%(objectname)"],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            [
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)"
            ],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            [
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)"
            ],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-command", "--buffer"],
            &format!("info {git_id}\ncontents {git_id}\nflush\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-command", "--buffer"],
            &format!("info {git_id}\ncontents {git_id}\nflush\n")
        )
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["cat-file", "-e", &git_id]),
        git_status(repo.path(), ["cat-file", "-e", &git_id])
    );

    git(repo.path(), ["repack", "-adq"]);
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-t", &git_id]),
        git(repo.path(), ["cat-file", "-t", &git_id])
    );
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-s", &git_id]),
        git(repo.path(), ["cat-file", "-s", &git_id])
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        ),
        git_with_stdin(
            repo.path(),
            ["cat-file", "--batch-check"],
            &format!("{git_id}\n")
        )
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["cat-file", "-e", &git_id]),
        git_status(repo.path(), ["cat-file", "-e", &git_id])
    );
}

#[test]
fn cat_file_mailmap_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "user.name", "Alias User"]);
    git(repo.path(), ["config", "user.email", "alias@example.com"]);
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "init"]);
    fs::write(
        repo.path().join(".mailmap"),
        b"Real User <real@example.com> Alias User <alias@example.com>\n",
    )
    .expect("write mailmap");
    git_with_env(repo.path(), ["tag", "-a", "v1", "-m", "tag message"]);

    let commit = git(repo.path(), ["rev-parse", "HEAD"]);
    let tag = git(repo.path(), ["rev-parse", "v1"]);

    for args in [
        vec!["cat-file", "-s", "--use-mailmap", commit.as_str()],
        vec!["cat-file", "-s", "--mailmap", commit.as_str()],
        vec![
            "cat-file",
            "-s",
            "--use-mailmap",
            "--no-mailmap",
            commit.as_str(),
        ],
        vec![
            "cat-file",
            "-s",
            "--mailmap",
            "--no-use-mailmap",
            commit.as_str(),
        ],
        vec!["cat-file", "-p", "--use-mailmap", commit.as_str()],
        vec!["cat-file", "-p", "--use-mailmap", tag.as_str()],
    ] {
        assert_eq!(
            command_any_output(zmin_bin(), repo.path(), &args, "zmin cat-file mailmap"),
            command_any_output("git", repo.path(), &args, "git cat-file mailmap")
        );
    }

    let batch_stdin = format!("{commit}\n");
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--batch-check", "--use-mailmap"],
            &batch_stdin,
            "zmin cat-file batch-check use-mailmap",
        ),
        command_any_output_with_stdin(
            "git",
            repo.path(),
            &["cat-file", "--batch-check", "--use-mailmap"],
            &batch_stdin,
            "git cat-file batch-check use-mailmap",
        )
    );
    assert_eq!(
        command_any_output_with_stdin(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--batch-command", "--use-mailmap"],
            &format!("info {commit}\ncontents {commit}\n"),
            "zmin cat-file batch-command use-mailmap",
        ),
        command_any_output_with_stdin(
            "git",
            repo.path(),
            &["cat-file", "--batch-command", "--use-mailmap"],
            &format!("info {commit}\ncontents {commit}\n"),
            "git cat-file batch-command use-mailmap",
        )
    );
}

#[test]
fn hash_object_matches_stock_git_for_documented_long_option_modes() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"alpha\n").expect("write fixture");

    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["hash-object", "--path=a.txt", "--stdin"],
            "stdin\n"
        ),
        git_with_stdin(
            repo.path(),
            ["hash-object", "--path=a.txt", "--stdin"],
            "stdin\n"
        )
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["hash-object", "--stdin-paths"], "a.txt\n"),
        git_with_stdin(repo.path(), ["hash-object", "--stdin-paths"], "a.txt\n")
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["hash-object", "--stdin-paths", "--no-filters"],
            "a.txt\n",
        ),
        git_with_stdin(
            repo.path(),
            ["hash-object", "--stdin-paths", "--no-filters"],
            "a.txt\n",
        )
    );
    assert_eq!(
        run_zmin_with_stdin(
            repo.path(),
            ["hash-object", "--literally", "--stdin"],
            "stdin\n"
        ),
        git_with_stdin(
            repo.path(),
            ["hash-object", "--literally", "--stdin"],
            "stdin\n"
        )
    );

    assert_eq!(
        run_zmin_failure_output(
            repo.path(),
            &["hash-object", "--no-filters", "--stdin", "--path=a.txt"],
        ),
        git_failure_output(
            repo.path(),
            &["hash-object", "--no-filters", "--stdin", "--path=a.txt"],
        )
    );
    assert_eq!(
        run_zmin_failure_output(
            repo.path(),
            &["hash-object", "--stdin-paths", "--path=a.txt"]
        ),
        git_failure_output(
            repo.path(),
            &["hash-object", "--stdin-paths", "--path=a.txt"]
        )
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["hash-object", "--stdin", "--stdin-paths"]),
        git_failure_output(repo.path(), &["hash-object", "--stdin", "--stdin-paths"])
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["hash-object", "--stdin-paths", "a.txt"]),
        git_failure_output(repo.path(), &["hash-object", "--stdin-paths", "a.txt"])
    );
}

#[test]
fn hash_object_validates_malformed_tree_commit_and_tag_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("blob.txt"), b"foo").expect("write blob");
    let blob = git(repo.path(), ["hash-object", "-w", "blob.txt"]);
    let blob_bytes = hex_to_bytes(&blob);

    let malformed_tree = repo.path().join("malformed-tree.bin");
    fs::write(&malformed_tree, b"abc").expect("write malformed tree");
    let malformed_args = [
        "hash-object",
        "-t",
        "tree",
        malformed_tree.to_str().expect("malformed tree path"),
    ];
    let zmin_malformed = command_any_output(
        zmin_bin(),
        repo.path(),
        &malformed_args,
        "zmin malformed tree",
    );
    let git_malformed =
        command_any_output("git", repo.path(), &malformed_args, "git malformed tree");
    assert_eq!(zmin_malformed.0, git_malformed.0);
    assert!(zmin_malformed.2.contains("too-short tree object"));

    let empty_name_tree = repo.path().join("empty-name-tree.bin");
    let mut empty_name_content = b"100644 \0".to_vec();
    empty_name_content.extend_from_slice(&blob_bytes);
    fs::write(&empty_name_tree, empty_name_content).expect("write empty-name tree");
    let empty_name_args = [
        "hash-object",
        "-t",
        "tree",
        empty_name_tree.to_str().expect("empty-name tree path"),
    ];
    let zmin_empty_name = command_any_output(
        zmin_bin(),
        repo.path(),
        &empty_name_args,
        "zmin empty-name tree",
    );
    let git_empty_name =
        command_any_output("git", repo.path(), &empty_name_args, "git empty-name tree");
    assert_eq!(zmin_empty_name.0, git_empty_name.0);
    assert!(zmin_empty_name.2.contains("empty filename in tree entry"));

    let duplicate_tree = repo.path().join("duplicate-tree.bin");
    let mut duplicate_content = b"100644 file\0".to_vec();
    duplicate_content.extend_from_slice(&blob_bytes);
    duplicate_content.extend_from_slice(b"100644 file\0");
    duplicate_content.extend_from_slice(&blob_bytes);
    fs::write(&duplicate_tree, duplicate_content).expect("write duplicate tree");
    let duplicate_args = [
        "hash-object",
        "-t",
        "tree",
        duplicate_tree.to_str().expect("duplicate tree path"),
    ];
    let zmin_duplicate = command_any_output(
        zmin_bin(),
        repo.path(),
        &duplicate_args,
        "zmin duplicate tree",
    );
    let git_duplicate =
        command_any_output("git", repo.path(), &duplicate_args, "git duplicate tree");
    assert_eq!(zmin_duplicate.0, git_duplicate.0);
    assert!(zmin_duplicate.2.contains("duplicateEntries"));

    let zmin_commit = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["hash-object", "-t", "commit", "--stdin"],
        "",
        "zmin corrupt commit",
    );
    let git_commit = command_any_output_with_stdin(
        "git",
        repo.path(),
        &["hash-object", "-t", "commit", "--stdin"],
        "",
        "git corrupt commit",
    );
    assert_eq!(zmin_commit.0, git_commit.0);
    assert!(zmin_commit.2.contains("corrupt commit"));

    let zmin_tag = command_any_output_with_stdin(
        zmin_bin(),
        repo.path(),
        &["hash-object", "-t", "tag", "--stdin"],
        "",
        "zmin corrupt tag",
    );
    let git_tag = command_any_output_with_stdin(
        "git",
        repo.path(),
        &["hash-object", "-t", "tag", "--stdin"],
        "",
        "git corrupt tag",
    );
    assert_eq!(zmin_tag.0, git_tag.0);
    assert!(zmin_tag.2.contains("corrupt tag"));
}

#[test]
fn cat_file_unknown_filter_names_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);
    let stdin = format!("{blob}\n");

    for filter in ["bad:name", "bad=name"] {
        let arg = format!("--filter={filter}");
        let args = ["cat-file", "--batch", arg.as_str()];
        assert_eq!(
            command_any_output_with_stdin_bytes(
                zmin_bin(),
                repo.path(),
                &args,
                stdin.as_bytes(),
                "zmin",
            ),
            command_any_output_with_stdin_bytes("git", repo.path(), &args, stdin.as_bytes(), "git",),
            "filter: {filter}"
        );
    }
}

#[test]
fn cat_file_known_unsupported_filters_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);
    let stdin = format!("{blob}\n");

    for filter in ["tree:1", "sparse:oid=deadbeef", "combine:blob:none+tree:1"] {
        let arg = format!("--filter={filter}");
        let args = ["cat-file", "--batch", arg.as_str()];
        assert_eq!(
            command_any_output_with_stdin_bytes(
                zmin_bin(),
                repo.path(),
                &args,
                stdin.as_bytes(),
                "zmin",
            ),
            command_any_output_with_stdin_bytes("git", repo.path(), &args, stdin.as_bytes(), "git",),
            "filter: {filter}"
        );
    }
}

#[test]
fn cat_file_sparse_path_filter_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "a.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);
    let stdin = format!("{blob}\n");
    let args = ["cat-file", "--batch", "--filter=sparse:path=foo"];

    assert_eq!(
        command_any_output_with_stdin_bytes(
            zmin_bin(),
            repo.path(),
            &args,
            stdin.as_bytes(),
            "zmin"
        ),
        command_any_output_with_stdin_bytes("git", repo.path(), &args, stdin.as_bytes(), "git",),
    );
}

#[test]
fn cat_file_promisor_remote_hydrates_missing_blob_on_demand() {
    let source = git_init();
    configure_identity(source.path());
    fs::write(source.path().join("a.txt"), b"one\n").expect("write tracked file");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "initial"]);
    let source_head = git(source.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    let remote = TempDir::new().expect("remote dir");
    git_args(
        source.path(),
        &[
            "clone",
            "--bare",
            ".",
            remote.path().to_str().expect("remote path utf8"),
        ],
    );

    let repo = init_promisor_work_repo(remote.path(), &source_head);
    let blob = git(repo.path(), ["rev-list", "--objects", "HEAD"])
        .lines()
        .find_map(|line| line.strip_suffix(" a.txt").map(str::to_owned))
        .expect("blob id");
    let object_path = loose_object_path(repo.path(), &blob);
    assert!(
        object_path.is_file(),
        "expected loose object at {}",
        object_path.display()
    );
    fs::remove_file(&object_path).expect("remove local blob");
    assert!(!object_path.exists());

    assert_eq!(run_zmin(repo.path(), ["cat-file", "-t", &blob]), "blob");
    assert!(
        pack_contains_object(repo.path(), &blob),
        "expected demand hydration pack to contain {blob}"
    );

    assert_eq!(run_zmin(repo.path(), ["cat-file", "blob", &blob]), "one");
    assert!(
        pack_contains_object(repo.path(), &blob),
        "expected typed-object demand hydration pack to contain {blob}"
    );
}

#[test]
fn cat_file_promisor_remote_lazy_fetch_writes_ref_in_want_trace() {
    let source = git_init();
    configure_identity(source.path());
    fs::write(source.path().join("a.txt"), b"one\n").expect("write tracked file");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "initial"]);
    let source_head = git(source.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    let remote = TempDir::new().expect("remote dir");
    git_args(
        source.path(),
        &[
            "clone",
            "--bare",
            ".",
            remote.path().to_str().expect("remote path utf8"),
        ],
    );

    let repo = init_promisor_work_repo(remote.path(), &source_head);
    let head = git(repo.path(), ["rev-parse", "HEAD"]);
    fs::remove_dir_all(repo.path().join(".git/objects")).expect("remove local objects");
    fs::create_dir_all(repo.path().join(".git/objects")).expect("restore objects dir");
    git(repo.path(), ["config", "core.repositoryformatversion", "1"]);
    git(repo.path(), ["config", "extensions.partialclone", "origin"]);
    git(repo.path(), ["config", "protocol.version", "2"]);
    git(remote.path(), ["config", "uploadpack.allowrefinwant", "1"]);
    let trace = repo.path().join("trace.packet");
    let trace_value = trace.to_str().expect("trace path");

    let output = command_output_with_env(
        zmin_bin(),
        repo.path(),
        &["cat-file", "-p", &head],
        &[("GIT_TRACE_PACKET", trace_value)],
        "zmin cat-file lazy fetch",
    );

    assert_eq!(output.0, 0, "zmin lazy fetch failed: {}", output.2);
    let trace_contents = fs::read_to_string(&trace).expect("trace file");
    assert!(trace_contents.contains("fetch< fetch=shallow wait-for-done ref-in-want"));
}

#[test]
fn cat_file_promisor_tree_fetch_does_not_hydrate_blob_closure() {
    let source = git_init();
    configure_identity(source.path());
    fs::write(source.path().join("a.txt"), b"one\n").expect("write tracked file");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "initial"]);
    let source_head = git(source.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    let remote = TempDir::new().expect("remote dir");
    git_args(
        source.path(),
        &[
            "clone",
            "--bare",
            ".",
            remote.path().to_str().expect("remote path utf8"),
        ],
    );

    let zmin_repo = init_promisor_work_repo(remote.path(), &source_head);
    let git_repo = init_promisor_work_repo(remote.path(), &source_head);
    let tree = git(zmin_repo.path(), ["rev-parse", "HEAD^{tree}"]);
    let blob = git(zmin_repo.path(), ["rev-parse", "HEAD:a.txt"]);
    for repo in [zmin_repo.path(), git_repo.path()] {
        fs::remove_dir_all(repo.join(".git/objects")).expect("remove local objects");
        fs::create_dir_all(repo.join(".git/objects")).expect("restore objects dir");
        git(repo, ["config", "core.repositoryformatversion", "1"]);
        git(repo, ["config", "extensions.partialclone", "origin"]);
        git(repo, ["config", "protocol.version", "2"]);
    }
    git(
        remote.path(),
        ["config", "uploadpack.allowanysha1inwant", "1"],
    );
    git(remote.path(), ["config", "uploadpack.allowfilter", "1"]);

    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", &tree]),
        git(git_repo.path(), ["cat-file", "-p", &tree])
    );
    assert!(
        !loose_object_path(zmin_repo.path(), &blob).is_file(),
        "blob should remain absent from the local object database after fetching the tree object"
    );
}

#[test]
fn cat_file_promisor_remote_tries_next_promisor_when_first_remote_lacks_object() {
    let source = git_init();
    configure_identity(source.path());
    fs::write(source.path().join("foo.txt"), b"foo\n").expect("write first source file");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "initial"]);
    let source_head = git(source.path(), ["rev-parse", "--abbrev-ref", "HEAD"]);

    let origin_remote = TempDir::new().expect("origin remote dir");
    git_args(
        source.path(),
        &[
            "clone",
            "--bare",
            ".",
            origin_remote.path().to_str().expect("origin path utf8"),
        ],
    );

    let repo = init_promisor_work_repo(origin_remote.path(), &source_head);

    let server2 = clone_repo_fixture(source.path());
    configure_identity(server2.path());
    fs::write(server2.path().join("bar.txt"), b"bar\n").expect("write second source file");
    git(server2.path(), ["add", "-A"]);
    git_with_env(server2.path(), ["commit", "-m", "bar"]);
    let server2_commit = git(server2.path(), ["rev-parse", "HEAD"]);

    let server2_remote = TempDir::new().expect("server2 remote dir");
    git_args(
        server2.path(),
        &[
            "clone",
            "--bare",
            ".",
            server2_remote.path().to_str().expect("server2 path utf8"),
        ],
    );
    git(
        server2_remote.path(),
        ["repack", "-a", "-d", "--write-bitmap-index"],
    );

    git(
        repo.path(),
        [
            "remote",
            "add",
            "server2",
            server2_remote.path().to_str().expect("server2 path utf8"),
        ],
    );
    git(repo.path(), ["config", "remote.origin.promisor", "true"]);
    git(repo.path(), ["config", "remote.server2.promisor", "true"]);
    git(repo.path(), ["fetch", "server2"]);

    fs::remove_dir_all(repo.path().join(".git/objects")).expect("remove local objects");
    fs::create_dir_all(repo.path().join(".git/objects")).expect("restore objects dir");

    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-p", &server2_commit]),
        git(repo.path(), ["cat-file", "-p", &server2_commit])
    );
    assert!(
        pack_contains_object(repo.path(), &server2_commit),
        "expected demand hydration pack to contain {server2_commit}"
    );

    let promisor_markers = fs::read_dir(repo.path().join(".git/objects/pack"))
        .expect("read pack dir")
        .map(|entry| entry.expect("pack entry").path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("promisor"))
        .count();
    assert_eq!(promisor_markers, 1);
}

#[test]
fn hash_object_write_prefers_bare_repo_at_current_directory() {
    let parent = git_init();
    let bare_path = parent.path().join("nested.git");
    run_zmin_args(
        parent.path(),
        &["init", "--bare", bare_path.to_str().expect("bare path")],
    );

    let object_id = run_zmin_with_stdin(&bare_path, ["hash-object", "-w", "--stdin"], "");
    assert_eq!(object_id, "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    assert!(
        bare_path
            .join("objects/e6/9de29bb2d1d6434b8b29ae775ad8c2e48c5391")
            .is_file()
    );
    assert!(
        !parent
            .path()
            .join(".git/objects/e6/9de29bb2d1d6434b8b29ae775ad8c2e48c5391")
            .exists()
    );
    assert_eq!(run_zmin(&bare_path, ["cat-file", "-s", &object_id]), "0");
}

#[test]
fn index_stage_object_paths_match_stock_git() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    fs::write(repo.path().join("b.txt"), b"hello\n").expect("write matching fixture");
    fs::write(repo.path().join("c.txt"), b"changed\n").expect("write changed fixture");
    git(repo.path(), ["add", "a.txt", "b.txt", "c.txt"]);

    for objectish in [":a.txt", ":0:a.txt"] {
        assert_eq!(
            run_zmin(repo.path(), ["rev-parse", objectish]),
            git(repo.path(), ["rev-parse", objectish])
        );
        assert_eq!(
            run_zmin(repo.path(), ["cat-file", "-p", objectish]),
            git(repo.path(), ["cat-file", "-p", objectish])
        );
    }
    assert_eq!(
        run_zmin_status(
            repo.path(),
            ["diff", "--raw", "--exit-code", ":a.txt", ":b.txt"]
        ),
        git_status(
            repo.path(),
            ["diff", "--raw", "--exit-code", ":a.txt", ":b.txt"]
        )
    );
    assert_eq!(
        run_zmin(repo.path(), ["diff", "--raw", ":a.txt", ":c.txt"]),
        git(repo.path(), ["diff", "--raw", ":a.txt", ":c.txt"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["diff", ":a.txt", ":c.txt"]),
        git(repo.path(), ["diff", ":a.txt", ":c.txt"])
    );
}

#[test]
fn ident_attribute_add_and_checkout_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join(".gitattributes"), b"*.i ident\n").expect("write attributes");
        fs::write(repo.join("id.i"), b"before\n$Id$\nafter\n").expect("write ident file");
    }

    git(git_repo.path(), ["add", "id.i"]);
    run_zmin(zmin_repo.path(), ["add", "id.i"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":id.i"]),
        git(git_repo.path(), ["cat-file", "-p", ":id.i"])
    );

    fs::remove_file(git_repo.path().join("id.i")).expect("remove git worktree file");
    fs::remove_file(zmin_repo.path().join("id.i")).expect("remove zmin worktree file");
    git(git_repo.path(), ["checkout", "--", "id.i"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "id.i"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("id.i")).expect("read zmin ident file"),
        fs::read_to_string(git_repo.path().join("id.i")).expect("read git ident file")
    );
}

#[test]
fn filter_attribute_add_and_checkout_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let rot13 = "tr 'A-Za-z' 'N-ZA-Mn-za-m'";
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "filter.rot13.clean", rot13]);
        git(repo, ["config", "filter.rot13.smudge", rot13]);
        fs::write(repo.join(".gitattributes"), b"*.t filter=rot13\n").expect("write attributes");
        fs::write(repo.join("message.t"), b"hello abc xyz\n").expect("write filtered file");
    }
    assert_eq!(
        run_zmin(zmin_repo.path(), ["hash-object", "message.t"]),
        git(git_repo.path(), ["hash-object", "message.t"])
    );

    git(git_repo.path(), ["add", "message.t"]);
    run_zmin(zmin_repo.path(), ["add", "message.t"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":message.t"]),
        git(git_repo.path(), ["cat-file", "-p", ":message.t"])
    );

    fs::remove_file(git_repo.path().join("message.t")).expect("remove git worktree file");
    fs::remove_file(zmin_repo.path().join("message.t")).expect("remove zmin worktree file");
    git(git_repo.path(), ["checkout", "--", "message.t"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "message.t"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.path().join("message.t")).expect("read zmin filtered file"),
        fs::read_to_string(git_repo.path().join("message.t")).expect("read git filtered file")
    );
}

#[test]
fn cat_file_filters_match_stock_git_for_eol_and_smudge_attributes() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "filter.rot13.clean", "cat"]);
    git(
        repo.path(),
        [
            "config",
            "filter.rot13.smudge",
            "tr 'A-Za-z' 'N-ZA-Mn-za-m'",
        ],
    );
    fs::write(
        repo.path().join(".gitattributes"),
        b"*.txt text eol=crlf\n*.rot filter=rot13\n",
    )
    .expect("write attributes");
    fs::write(repo.path().join("line.txt"), b"one\ntwo\n").expect("write eol file");
    fs::write(repo.path().join("message.rot"), b"hello abc xyz\n").expect("write filtered file");
    git(repo.path(), ["add", "."]);
    git_with_env(repo.path(), ["commit", "-m", "filters"]);

    let text_blob = git(repo.path(), ["rev-parse", "HEAD:line.txt"]);
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--filters", "HEAD:line.txt"]
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--filters", "HEAD:line.txt"]
        )
    );
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--filters", "--path=line.txt", &text_blob],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--filters", "--path=line.txt", &text_blob],
        )
    );

    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--filters", "HEAD:message.rot"],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--filters", "HEAD:message.rot"],
        )
    );
}

#[test]
fn cat_file_textconv_matches_stock_git_for_diff_driver_attributes() {
    let repo = git_init();
    configure_identity(repo.path());
    git(
        repo.path(),
        ["config", "diff.upper.textconv", "tr 'a-z' 'A-Z' <"],
    );
    fs::write(repo.path().join(".gitattributes"), b"*.bin diff=upper\n").expect("write attributes");
    fs::write(repo.path().join("payload.bin"), b"hello abc\n").expect("write payload");
    fs::write(repo.path().join("plain.txt"), b"plain\n").expect("write plain");
    git(repo.path(), ["add", "."]);
    git_with_env(repo.path(), ["commit", "-m", "textconv"]);

    let blob = git(repo.path(), ["rev-parse", "HEAD:payload.bin"]);
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--textconv", "HEAD:payload.bin"],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--textconv", "HEAD:payload.bin"],
        )
    );
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--textconv", "--path=payload.bin", &blob],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--textconv", "--path=payload.bin", &blob],
        )
    );
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            repo.path(),
            &["cat-file", "--textconv", "HEAD:plain.txt"],
        ),
        command_stdout_bytes(
            "git",
            repo.path(),
            &["cat-file", "--textconv", "HEAD:plain.txt"],
        )
    );
}

#[test]
#[cfg(unix)]
fn process_filter_attribute_add_and_checkout_match_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    let helper = git_repo.path().join("process-filter.pl");
    fs::write(
        &helper,
        r#"use strict;
use warnings;
binmode STDIN;
binmode STDOUT;
$| = 1;
open my $log, ">>", $ARGV[0] or die "log";
sub readpkt {
    my $header = "";
    my $read = read(STDIN, $header, 4);
    return undef unless defined($read) && $read == 4;
    my $len = hex($header);
    return "" if $len == 0;
    my $payload = "";
    read(STDIN, $payload, $len - 4) == $len - 4 or die "short read";
    return $payload;
}
sub readtext {
    my $value = readpkt();
    return undef unless defined $value;
    $value =~ s/\n$//;
    return $value;
}
sub writepkt {
    my ($payload) = @_;
    printf "%04x%s", length($payload) + 4, $payload;
}
sub flushpkt { print "0000"; }
sub rot13 {
    my ($value) = @_;
    $value =~ tr/A-Za-z/N-ZA-Mn-za-m/;
    return $value;
}
print $log "START\n";
die "client" unless readtext() eq "git-filter-client";
die "version" unless readtext() eq "version=2";
die "flush" unless readtext() eq "";
writepkt("git-filter-server");
writepkt("version=2");
flushpkt();
while ((my $cap = readtext()) ne "") {}
writepkt("capability=clean");
writepkt("capability=smudge");
flushpkt();
print $log "init handshake complete\n";
while (1) {
    my $command = readtext();
    last unless defined $command;
    $command =~ s/^command=// or die "command";
    my $path = readtext();
    $path =~ s/^pathname=// or die "path";
    while ((my $meta = readtext()) ne "") {}
    my $content = "";
    while ((my $packet = readpkt()) ne "") { $content .= $packet; }
    print $log "IN: $command $path\n";
    my $out = rot13($content);
    writepkt("status=success");
    flushpkt();
    while (length($out) > 0) {
        my $chunk = substr($out, 0, 65516, "");
        writepkt($chunk);
    }
    flushpkt();
    flushpkt();
}
print $log "STOP\n";
"#,
    )
    .expect("write process filter helper");
    let command = format!(
        "perl {} debug.log",
        shell_quote_for_test(&helper.to_string_lossy())
    );
    for repo in [git_repo.path(), zmin_repo.path()] {
        git(repo, ["config", "filter.protocol.process", &command]);
        git(repo, ["config", "filter.protocol.required", "true"]);
        fs::write(repo.join(".gitattributes"), b"*.r filter=protocol\n").expect("write attributes");
        fs::write(repo.join("one.r"), b"hello abc\n").expect("write one");
        fs::write(repo.join("two.r"), b"xyz world\n").expect("write two");
    }
    assert_eq!(
        run_zmin(zmin_repo.path(), ["hash-object", "one.r"]),
        git(git_repo.path(), ["hash-object", "one.r"])
    );
    let _ = fs::remove_file(git_repo.path().join("debug.log"));
    let _ = fs::remove_file(zmin_repo.path().join("debug.log"));

    git(git_repo.path(), ["add", "."]);
    run_zmin(zmin_repo.path(), ["add", "."]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":one.r"]),
        git(git_repo.path(), ["cat-file", "-p", ":one.r"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "-p", ":two.r"]),
        git(git_repo.path(), ["cat-file", "-p", ":two.r"])
    );
    let zmin_log = fs::read_to_string(zmin_repo.path().join("debug.log")).expect("read log");
    assert_eq!(zmin_log.matches("START\n").count(), 1);
    assert_eq!(zmin_log.matches("STOP\n").count(), 1);
    assert!(zmin_log.contains("IN: clean one.r\n"));
    assert!(zmin_log.contains("IN: clean two.r\n"));

    fs::remove_file(git_repo.path().join("one.r")).expect("remove git one");
    fs::remove_file(git_repo.path().join("two.r")).expect("remove git two");
    fs::remove_file(zmin_repo.path().join("one.r")).expect("remove zmin one");
    fs::remove_file(zmin_repo.path().join("two.r")).expect("remove zmin two");
    fs::remove_file(zmin_repo.path().join("debug.log")).expect("remove zmin log");
    git(git_repo.path(), ["checkout", "--", "one.r", "two.r"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "one.r", "two.r"]);
    assert_eq!(
        fs::read(zmin_repo.path().join("one.r")).expect("read zmin one"),
        fs::read(git_repo.path().join("one.r")).expect("read git one")
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("two.r")).expect("read zmin two"),
        fs::read(git_repo.path().join("two.r")).expect("read git two")
    );
    let zmin_log = fs::read_to_string(zmin_repo.path().join("debug.log")).expect("read log");
    assert_eq!(zmin_log.matches("START\n").count(), 1);
    assert_eq!(zmin_log.matches("STOP\n").count(), 1);
    assert!(zmin_log.contains("IN: smudge one.r\n"));
    assert!(zmin_log.contains("IN: smudge two.r\n"));
}

#[test]
#[cfg(unix)]
fn add_with_clean_filter_that_does_not_read_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join(".gitattributes"), b"big filter=epipe\n").expect("write attributes");
        fs::write(repo.join("big"), vec![b'a'; 128 * 1024 + 1]).expect("write large fixture");
    }

    git(
        git_repo.path(),
        ["config", "filter.epipe.clean", "echo xyzzy"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "filter.epipe.clean", "echo xyzzy"],
    );

    let stock = command_any_output("git", git_repo.path(), &["add", "big"], "stock git add big");
    let zmin = command_any_output(
        zmin_bin(),
        zmin_repo.path(),
        &["add", "big"],
        "zmin add big",
    );

    assert_eq!(zmin.0, stock.0, "stderr: {}", zmin.2);
    assert_eq!(zmin.1, stock.1);
    assert_eq!(zmin.2, stock.2);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["cat-file", "blob", ":big"]),
        git(git_repo.path(), ["cat-file", "blob", ":big"])
    );
}

#[test]
#[cfg(unix)]
fn process_filter_subcommand_metadata_matches_stock_git() {
    const FIXED_ENV: [(&str, &str); 6] = [
        ("GIT_AUTHOR_NAME", "Bench"),
        ("GIT_AUTHOR_EMAIL", "bench@example.test"),
        ("GIT_AUTHOR_DATE", "1700000000 +0000"),
        ("GIT_COMMITTER_NAME", "Bench"),
        ("GIT_COMMITTER_EMAIL", "bench@example.test"),
        ("GIT_COMMITTER_DATE", "1700000000 +0000"),
    ];

    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let helper_dir = TempDir::new().expect("helper tempdir");
    let helper = helper_dir.path().join("process-filter-meta.pl");
    fs::write(
        &helper,
        r#"use strict;
use warnings;
binmode STDIN;
binmode STDOUT;
$| = 1;
open my $log, ">>", $ARGV[0] or die "log";
sub readpkt {
    my $header = "";
    my $read = read(STDIN, $header, 4);
    return undef unless defined($read) && $read == 4;
    my $len = hex($header);
    return "" if $len == 0;
    my $payload = "";
    read(STDIN, $payload, $len - 4) == $len - 4 or die "short read";
    return $payload;
}
sub readtext {
    my $value = readpkt();
    return undef unless defined $value;
    $value =~ s/\n$//;
    return $value;
}
sub writepkt {
    my ($payload) = @_;
    printf "%04x%s", length($payload) + 4, $payload;
}
sub flushpkt { print "0000"; }
sub rot13 {
    my ($value) = @_;
    $value =~ tr/A-Za-z/N-ZA-Mn-za-m/;
    return $value;
}
print $log "START\n";
die "client" unless readtext() eq "git-filter-client";
die "version" unless readtext() eq "version=2";
die "flush" unless readtext() eq "";
writepkt("git-filter-server");
writepkt("version=2");
flushpkt();
while ((my $cap = readtext()) ne "") {}
writepkt("capability=clean");
writepkt("capability=smudge");
flushpkt();
print $log "init handshake complete\n";
while (1) {
    my $command = readtext();
    last unless defined $command;
    $command =~ s/^command=// or die "command";
    my $path = readtext();
    $path =~ s/^pathname=// or die "path";
    my @meta;
    while ((my $meta = readtext()) ne "") {
        push @meta, $meta;
    }
    my $content = "";
    while ((my $packet = readpkt()) ne "") {
        $content .= $packet;
    }
    my $size = length($content);
    my $dots = "." x (($size + 65515) / 65516);
    print $log "IN: $command $path";
    print $log " " . join(" ", @meta) if @meta;
    print $log " $size [OK] -- OUT: $size $dots [OK]\n";
    my $out = rot13($content);
    writepkt("status=success");
    flushpkt();
    while (length($out) > 0) {
        my $chunk = substr($out, 0, 65516, "");
        writepkt($chunk);
    }
    flushpkt();
    flushpkt();
}
print $log "STOP\n";
"#,
    )
    .expect("write metadata helper");

    let command = format!(
        "perl {} debug.log",
        shell_quote_for_test(&helper.to_string_lossy())
    );

    let run_git = |cwd: &std::path::Path, args: &[&str], label: &str| {
        command_output_with_env("git", cwd, args, &FIXED_ENV, label)
    };
    let run_zmin_env = |cwd: &std::path::Path, args: &[&str], label: &str| {
        command_output_with_env(zmin_bin(), cwd, args, &FIXED_ENV, label)
    };
    let compare_logs = |stage: &str| {
        let git_log = fs::read_to_string(git_repo.path().join("debug.log")).expect("read git log");
        let zmin_log =
            fs::read_to_string(zmin_repo.path().join("debug.log")).expect("read zmin log");
        assert_eq!(zmin_log, git_log, "stage: {stage}");
    };
    let clear_logs = || {
        let _ = fs::remove_file(git_repo.path().join("debug.log"));
        let _ = fs::remove_file(zmin_repo.path().join("debug.log"));
    };

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join(".gitattributes"), b"*.r filter=protocol\n").expect("write attrs");
        fs::write(repo.join("test.r"), b"uryyb grfg.e\n").expect("write test");
        fs::write(repo.join("test2.r"), b"uryyb grfg2.e\n").expect("write test2");
        fs::create_dir_all(repo.join("testsubdir")).expect("mkdir subdir");
        fs::write(repo.join("testsubdir/test3 'sq',$x=.r"), b"uryyb fd\n").expect("write test3");
        fs::write(repo.join("test4-empty.r"), b"").expect("write empty");
    }
    git(
        git_repo.path(),
        ["config", "filter.protocol.process", &command],
    );
    git(
        git_repo.path(),
        ["config", "filter.protocol.required", "true"],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "filter.protocol.process", &command],
    );
    run_zmin(
        zmin_repo.path(),
        ["config", "filter.protocol.required", "true"],
    );

    run_git(git_repo.path(), &["add", ".gitattributes"], "git add attrs");
    run_zmin_env(
        zmin_repo.path(),
        &["add", ".gitattributes"],
        "zmin add attrs",
    );
    run_git(
        git_repo.path(),
        &["commit", "-m", "test commit 1"],
        "git commit 1",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["commit", "-m", "test commit 1"],
        "zmin commit 1",
    );
    run_git(
        git_repo.path(),
        &["branch", "-M", "main"],
        "git branch -M main",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["branch", "-M", "main"],
        "zmin branch -M main",
    );
    run_git(
        git_repo.path(),
        &["branch", "empty-branch"],
        "git branch empty",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["branch", "empty-branch"],
        "zmin branch empty",
    );

    clear_logs();
    run_git(git_repo.path(), &["add", "."], "git add fixtures");
    run_zmin_env(zmin_repo.path(), &["add", "."], "zmin add fixtures");
    compare_logs("add");
    run_git(
        git_repo.path(),
        &["commit", "-m", "test commit 2"],
        "git commit 2",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["commit", "-m", "test commit 2"],
        "zmin commit 2",
    );

    fs::copy(
        git_repo.path().join("test.r"),
        git_repo.path().join("test5.r"),
    )
    .expect("copy git");
    fs::copy(
        zmin_repo.path().join("test.r"),
        zmin_repo.path().join("test5.r"),
    )
    .expect("copy zmin");
    run_git(git_repo.path(), &["add", "test5.r"], "git add test5");
    run_zmin_env(zmin_repo.path(), &["add", "test5.r"], "zmin add test5");
    run_git(
        git_repo.path(),
        &["commit", "-m", "test commit 3"],
        "git commit 3",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["commit", "-m", "test commit 3"],
        "zmin commit 3",
    );
    run_git(
        git_repo.path(),
        &["checkout", "empty-branch"],
        "git checkout empty",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["checkout", "empty-branch"],
        "zmin checkout empty",
    );

    clear_logs();
    run_git(
        git_repo.path(),
        &["rebase", "--onto", "empty-branch", "main^^", "main"],
        "git rebase",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["rebase", "--onto", "empty-branch", "main^^", "main"],
        "zmin rebase",
    );
    compare_logs("rebase");
    let git_main = run_git(
        git_repo.path(),
        &["rev-parse", "--verify", "main"],
        "git main2",
    )
    .1;
    let zmin_main = run_zmin_env(
        zmin_repo.path(),
        &["rev-parse", "--verify", "main"],
        "zmin main2",
    )
    .1;

    run_git(
        git_repo.path(),
        &["reset", "--hard", "empty-branch"],
        "git reset empty",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["reset", "--hard", "empty-branch"],
        "zmin reset empty",
    );
    clear_logs();
    run_git(
        git_repo.path(),
        &["reset", "--hard", git_main.as_str()],
        "git reset main",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["reset", "--hard", zmin_main.as_str()],
        "zmin reset main",
    );
    compare_logs("reset-main");

    run_git(
        git_repo.path(),
        &["branch", "old-main", git_main.as_str()],
        "git branch old-main",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["branch", "old-main", zmin_main.as_str()],
        "zmin branch old-main",
    );
    run_git(
        git_repo.path(),
        &["reset", "--hard", "empty-branch"],
        "git reset empty 2",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["reset", "--hard", "empty-branch"],
        "zmin reset empty 2",
    );
    clear_logs();
    run_git(
        git_repo.path(),
        &["reset", "--hard", "old-main"],
        "git reset old-main",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["reset", "--hard", "old-main"],
        "zmin reset old-main",
    );
    compare_logs("reset-old-main");

    run_git(
        git_repo.path(),
        &["checkout", "-b", "merge", "empty-branch"],
        "git checkout merge",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["checkout", "-b", "merge", "empty-branch"],
        "zmin checkout merge",
    );
    run_git(
        git_repo.path(),
        &["branch", "-f", "main", git_main.as_str()],
        "git force main",
    );
    run_zmin_env(
        zmin_repo.path(),
        &["branch", "-f", "main", zmin_main.as_str()],
        "zmin force main",
    );
    clear_logs();
    run_git(git_repo.path(), &["merge", "main"], "git merge main");
    run_zmin_env(zmin_repo.path(), &["merge", "main"], "zmin merge main");
    compare_logs("merge-main");

    clear_logs();
    let _git_archive = command_stdout_bytes("git", git_repo.path(), &["archive", "main"]);
    let _zmin_archive = command_stdout_bytes(zmin_bin(), zmin_repo.path(), &["archive", "main"]);
    compare_logs("archive-main");

    let git_tree = run_git(
        git_repo.path(),
        &["rev-parse", &format!("{git_main}^{{tree}}")],
        "git tree rev-parse",
    );
    let zmin_tree = run_zmin_env(
        zmin_repo.path(),
        &["rev-parse", &format!("{zmin_main}^{{tree}}")],
        "zmin tree rev-parse",
    );
    clear_logs();
    let _git_tree_archive =
        command_stdout_bytes("git", git_repo.path(), &["archive", git_tree.1.as_str()]);
    let _zmin_tree_archive = command_stdout_bytes(
        zmin_bin(),
        zmin_repo.path(),
        &["archive", zmin_tree.1.as_str()],
    );
    compare_logs("archive-tree");
}

#[test]
#[cfg(unix)]
fn lfs_process_filter_pointer_workflow_matches_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    let helper_dir = TempDir::new().expect("helper tempdir");
    let helper = helper_dir.path().join("fake-lfs-filter.py");
    fs::write(
        &helper,
        r#"#!/usr/bin/env python3
import hashlib
import pathlib
import sys

store = pathlib.Path(sys.argv[1])
log_path = pathlib.Path(sys.argv[2])
store.mkdir(parents=True, exist_ok=True)
log_path.parent.mkdir(parents=True, exist_ok=True)

def read_exact(size):
    data = sys.stdin.buffer.read(size)
    if len(data) != size:
        raise SystemExit("short read")
    return data

def readpkt():
    header = sys.stdin.buffer.read(4)
    if not header:
        return None
    if len(header) != 4:
        raise SystemExit("short header")
    length = int(header, 16)
    if length == 0:
        return b""
    return read_exact(length - 4)

def readtext():
    value = readpkt()
    if value is None:
        return None
    if value == b"":
        return ""
    return value.decode("utf-8").rstrip("\n")

def writepkt(payload: bytes):
    sys.stdout.buffer.write(f"{len(payload) + 4:04x}".encode("ascii"))
    sys.stdout.buffer.write(payload)

def writetext(text: str):
    writepkt(text.encode("utf-8"))

def flushpkt():
    sys.stdout.buffer.write(b"0000")

def log(text: str):
    with log_path.open("a", encoding="utf-8") as handle:
        handle.write(text + "\n")

def read_packetized_content():
    chunks = []
    while True:
        payload = readpkt()
        if payload is None:
            raise SystemExit("unexpected eof")
        if payload == b"":
            return b"".join(chunks)
        chunks.append(payload)

def lfs_pointer(content: bytes) -> bytes:
    oid = hashlib.sha256(content).hexdigest()
    path = store / oid
    if not path.exists():
        path.write_bytes(content)
    return (
        "version https://git-lfs.github.com/spec/v1\n"
        f"oid sha256:{oid}\n"
        f"size {len(content)}\n"
    ).encode("utf-8")

def lfs_smudge(pointer: bytes) -> bytes:
    oid = None
    for line in pointer.decode("utf-8").splitlines():
        if line.startswith("oid sha256:"):
            oid = line.split(":", 1)[1]
            break
    if oid is None:
        return pointer
    return (store / oid).read_bytes()

assert readtext() == "git-filter-client"
assert readtext() == "version=2"
assert readtext() == ""
writetext("git-filter-server")
writetext("version=2")
flushpkt()
sys.stdout.buffer.flush()
while True:
    line = readtext()
    if line == "":
        break
    if line is None:
        raise SystemExit("missing capabilities")
writetext("capability=clean")
writetext("capability=smudge")
flushpkt()
sys.stdout.buffer.flush()
log("START")
while True:
    command = readtext()
    if command is None:
        break
    command = command.removeprefix("command=")
    pathname = readtext().removeprefix("pathname=")
    while True:
        meta = readtext()
        if meta == "":
            break
    content = read_packetized_content()
    log(f"{command} {pathname}")
    if command == "clean":
        out = lfs_pointer(content)
    elif command == "smudge":
        out = lfs_smudge(content)
    else:
        writetext("status=abort")
        flushpkt()
        flushpkt()
        sys.stdout.buffer.flush()
        continue
    writetext("status=success")
    flushpkt()
    if out:
        writepkt(out)
    flushpkt()
    flushpkt()
    sys.stdout.buffer.flush()
log("STOP")
"#,
    )
    .expect("write fake lfs helper");

    let git_command = format!(
        "python3 {} {} {}",
        shell_quote_for_test(&helper.to_string_lossy()),
        shell_quote_for_test(&git_repo.path().join("lfs-store").to_string_lossy()),
        shell_quote_for_test(&git_repo.path().join("lfs.log").to_string_lossy()),
    );
    let zmin_command = format!(
        "python3 {} {} {}",
        shell_quote_for_test(&helper.to_string_lossy()),
        shell_quote_for_test(&zmin_repo.path().join("lfs-store").to_string_lossy()),
        shell_quote_for_test(&zmin_repo.path().join("lfs.log").to_string_lossy()),
    );

    let payload = b"\x00zmin-lfs-payload\xff\nsecond-line\n";
    for (repo, command) in [
        (git_repo.path(), git_command.as_str()),
        (zmin_repo.path(), zmin_command.as_str()),
    ] {
        git(repo, ["config", "filter.lfs.process", command]);
        git(repo, ["config", "filter.lfs.required", "true"]);
        fs::write(
            repo.join(".gitattributes"),
            b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("write lfs attributes");
        fs::write(repo.join("asset.bin"), payload).expect("write asset");
    }

    git(git_repo.path(), ["add", "."]);
    run_zmin(zmin_repo.path(), ["add", "."]);
    let git_pointer =
        command_stdout_bytes("git", git_repo.path(), &["cat-file", "-p", ":asset.bin"]);
    let zmin_pointer = command_stdout_bytes(
        zmin_bin(),
        zmin_repo.path(),
        &["cat-file", "-p", ":asset.bin"],
    );
    assert_eq!(zmin_pointer, git_pointer);
    assert!(
        String::from_utf8_lossy(&zmin_pointer)
            .starts_with("version https://git-lfs.github.com/spec/v1\n"),
        "expected LFS pointer, got {}",
        String::from_utf8_lossy(&zmin_pointer)
    );

    git_with_env(git_repo.path(), ["commit", "-m", "lfs asset"]);
    run_zmin(zmin_repo.path(), ["commit", "-m", "lfs asset"]);

    fs::remove_file(git_repo.path().join("asset.bin")).expect("remove git asset");
    fs::remove_file(zmin_repo.path().join("asset.bin")).expect("remove zmin asset");
    git(git_repo.path(), ["checkout", "--", "asset.bin"]);
    run_zmin(zmin_repo.path(), ["checkout", "--", "asset.bin"]);
    assert_eq!(
        fs::read(git_repo.path().join("asset.bin")).expect("read git asset"),
        payload
    );
    assert_eq!(
        fs::read(zmin_repo.path().join("asset.bin")).expect("read zmin asset"),
        payload
    );
    assert_eq!(
        command_stdout_bytes(
            zmin_bin(),
            zmin_repo.path(),
            &["cat-file", "--filters", "HEAD:asset.bin"]
        ),
        command_stdout_bytes(
            "git",
            git_repo.path(),
            &["cat-file", "--filters", "HEAD:asset.bin"]
        )
    );

    let zmin_log = fs::read_to_string(zmin_repo.path().join("lfs.log")).expect("read zmin lfs log");
    assert!(zmin_log.contains("clean asset.bin\n"));
    assert!(zmin_log.contains("smudge asset.bin\n"));
}

#[cfg(unix)]
fn shell_quote_for_test(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[test]
fn cat_file_resolves_reflog_selector_for_ref_name_ending_with_at_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("hello"), b"hello\n").expect("write hello");
    git(repo.path(), ["add", "hello"]);
    git_with_env(repo.path(), ["commit", "-m", "hello"]);
    run_zmin(repo.path(), ["branch", "foo@"]);

    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-p", "foo@@{0}:hello"]),
        git(repo.path(), ["cat-file", "-p", "HEAD:hello"])
    );
}

#[test]
fn show_matches_stock_git_for_raw_commits_trees_blobs_and_tags() {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["config", "tag.gpgSign", "false"]);
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write a");
    fs::write(repo.path().join("dir/b.txt"), b"nested\n").expect("write b");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git_with_env(repo.path(), ["tag", "-a", "v1", "-m", "tag message"]);

    for args in [
        ["show", "HEAD:a.txt"].as_slice(),
        ["show", "HEAD^{tree}"].as_slice(),
        ["show", "HEAD"].as_slice(),
        ["show", "--oneline", "HEAD"].as_slice(),
        ["show", "--format=%H", "HEAD"].as_slice(),
        ["show", "--stat", "HEAD"].as_slice(),
        ["show", "--numstat", "--format=%H", "HEAD"].as_slice(),
        ["show", "--shortstat", "HEAD"].as_slice(),
        ["show", "--raw", "--format=%H", "HEAD"].as_slice(),
        ["show", "--summary", "--format=%H", "HEAD"].as_slice(),
        ["show", "--name-only", "--format=%H", "HEAD"].as_slice(),
        ["show", "--name-status", "--format=%H", "HEAD"].as_slice(),
        ["show", "--no-patch", "--format=raw", "HEAD"].as_slice(),
        ["show", "--format=raw", "HEAD"].as_slice(),
        ["show", "--no-patch", "--format=%H", "HEAD"].as_slice(),
        ["show", "--no-patch", "--pretty=format:%an <%ae>", "HEAD"].as_slice(),
        ["show", "--no-patch", "--oneline", "HEAD"].as_slice(),
        ["show", "--no-patch", "HEAD"].as_slice(),
        ["show", "--no-patch", "--format=raw", "v1"].as_slice(),
        ["show", "v1"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn treeish_path_resolution_and_ls_tree_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    fs::write(repo.path().join("README.md"), b"hello\n").expect("write readme");
    fs::write(repo.path().join("src/main.rs"), b"fn main() {}\n").expect("write source");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    fs::write(repo.path().join("README.md"), b"hello again\n").expect("modify readme");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);

    for args in [
        ["rev-parse", "HEAD^{tree}"].as_slice(),
        ["rev-parse", "HEAD:src/main.rs"].as_slice(),
        ["cat-file", "-p", "HEAD:src/main.rs"].as_slice(),
        ["cat-file", "-p", "HEAD^{tree}"].as_slice(),
        ["rev-parse", "HEAD~1"].as_slice(),
        ["rev-parse", "HEAD~1^{tree}"].as_slice(),
        ["ls-tree", "HEAD"].as_slice(),
        ["ls-tree", "HEAD^{tree}"].as_slice(),
        ["ls-tree", "-r", "--name-only", "HEAD"].as_slice(),
        ["ls-tree", "-r", "-t", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn ls_tree_extended_options_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("src/deep")).expect("create nested dirs");
    fs::write(repo.path().join("README.md"), b"hello\n").expect("write readme");
    fs::write(repo.path().join("src/main.rs"), b"fn main() {}\n").expect("write source");
    fs::write(repo.path().join("src/deep/lib.rs"), b"pub fn lib() {}\n").expect("write nested");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    for args in [
        ["ls-tree", "-d", "HEAD", "src"].as_slice(),
        ["ls-tree", "-l", "HEAD"].as_slice(),
        ["ls-tree", "--object-only", "HEAD"].as_slice(),
        ["ls-tree", "--abbrev=10", "HEAD"].as_slice(),
        ["ls-tree", "--name-status", "HEAD"].as_slice(),
        ["ls-tree", "--format=%(objectname) %(path)", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    assert_eq!(
        command_stdout_bytes(zmin_bin(), repo.path(), &["ls-tree", "-z", "HEAD"]),
        command_stdout_bytes("git", repo.path(), &["ls-tree", "-z", "HEAD"])
    );
}

#[test]
fn ls_tree_subdir_full_name_and_full_tree_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir/sub")).expect("create nested dirs");
    fs::write(repo.path().join("README.md"), b"root\n").expect("write root");
    fs::write(repo.path().join("dir/file.txt"), b"child\n").expect("write child");
    fs::write(repo.path().join("dir/sub/nested.txt"), b"nested\n").expect("write nested");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    let cwd = repo.path().join("dir");
    for args in [
        ["ls-tree", "HEAD"].as_slice(),
        ["ls-tree", "--full-name", "HEAD"].as_slice(),
        ["ls-tree", "--full-tree", "HEAD"].as_slice(),
        ["ls-tree", "-r", "HEAD"].as_slice(),
        ["ls-tree", "-r", "--full-name", "HEAD"].as_slice(),
        ["ls-tree", "-r", "--full-tree", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            command_stdout_bytes(zmin_bin(), &cwd, args),
            command_stdout_bytes("git", &cwd, args),
            "args: {args:?}"
        );
    }
}

#[test]
fn unpack_file_matches_stock_git_blob_behavior() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    let blob = git(repo.path(), ["hash-object", "-w", "a.txt"]);

    let zmin_path = run_zmin(repo.path(), ["unpack-file", &blob]);
    assert!(zmin_path.starts_with(".merge_file_"));
    assert_eq!(
        fs::read(repo.path().join(&zmin_path)).expect("read zmin unpacked file"),
        b"hello\n"
    );

    let git_path = git(repo.path(), ["unpack-file", &blob]);
    assert!(git_path.starts_with(".merge_file_"));
    assert_eq!(
        fs::read(repo.path().join(&git_path)).expect("read git unpacked file"),
        b"hello\n"
    );

    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    let commit = git(repo.path(), ["rev-parse", "HEAD"]);
    assert_eq!(
        run_zmin_status(repo.path(), ["unpack-file", &commit]),
        git_status(repo.path(), ["unpack-file", &commit])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["unpack-file", "deadbeef"]),
        git_status(repo.path(), ["unpack-file", "deadbeef"])
    );
}

#[test]
fn show_index_matches_stock_git_for_pack_index_stdin() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["repack", "-adq"]);
    let idx = fs::read(first_pack_index(repo.path())).expect("read pack index");

    assert_eq!(
        run_zmin_with_stdin_bytes(repo.path(), ["show-index"], &idx),
        git_with_stdin_bytes(repo.path(), ["show-index"], &idx)
    );
}

#[test]
fn show_index_option_order_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["repack", "-adq"]);
    let idx = fs::read(first_pack_index(repo.path())).expect("read pack index");

    for args in [
        &[
            "show-index",
            "--object-format=sha256",
            "--object-format=sha1",
        ][..],
        &["show-index", "--object-format=bogus", "--no-object-format"][..],
        &["show-index", "--no-object-format", "--object-format=bogus"][..],
        &["show-index", "--object-format=sha256"][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--object-format=sha256",
        ][..],
        &["show-index", "--no-object-format", "--object-format=sha256"][..],
        &[
            "show-index",
            "--object-format",
            "sha1",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--object-format",
            "sha1",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format=sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--no-object-format",
            "--object-format=sha1",
        ][..],
        &["show-index", "--object-format=sha256", "--no-object-format"][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format=bogus",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format=bogus",
            "--no-object-format",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--object-format",
            "sha1",
            "--object-format",
            "sha1",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--object-format=sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format",
            "sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format",
            "sha1",
            "--no-object-format",
            "--object-format",
            "sha1",
        ][..],
        &[
            "show-index",
            "--object-format=sha256",
            "--object-format=sha256",
        ][..],
        &[
            "show-index",
            "--object-format=sha256",
            "--object-format=sha256",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format=bogus",
            "--no-object-format",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format=sha1",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--no-object-format",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format=sha256",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format",
            "sha1",
            "--object-format",
            "sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format",
            "sha1",
            "--object-format",
            "sha1",
            "--no-object-format",
            "--object-format",
            "sha1",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--object-format",
            "sha1",
            "--object-format",
            "sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--object-format=sha1",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--object-format=sha256",
            "--object-format=sha256",
            "--object-format=sha256",
        ][..],
        &[
            "show-index",
            "--object-format=sha256",
            "--object-format=sha256",
            "--no-object-format",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--object-format=bogus",
            "--no-object-format",
            "--object-format=sha1",
            "--no-object-format",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--no-object-format",
            "--object-format=sha1",
        ][..],
        &[
            "show-index",
            "--no-object-format",
            "--no-object-format",
            "--object-format=sha256",
        ][..],
        &[
            "show-index",
            "--object-format=sha1",
            "--no-object-format",
            "--object-format=sha256",
            "--no-object-format",
        ][..],
    ] {
        assert_eq!(
            command_any_output_with_stdin_bytes(zmin_bin(), repo.path(), &args, &idx, "zmin"),
            command_any_output_with_stdin_bytes("git", repo.path(), &args, &idx, "git")
        );
    }
}

#[test]
fn show_index_rejects_unsupported_pack_index_version_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["repack", "-adq"]);
    let idx = rewrite_pack_index_version(&first_pack_index(repo.path()), 3);

    assert_eq!(
        command_any_output_with_stdin_bytes(zmin_bin(), repo.path(), &["show-index"], &idx, "zmin"),
        command_any_output_with_stdin_bytes("git", repo.path(), &["show-index"], &idx, "git")
    );
}

#[test]
fn update_server_info_matches_stock_git_for_bare_repo() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(dir.path(), ["init", "-b", "main", "source"]);
    configure_identity(&source);
    fs::write(source.join("a.txt"), b"hello\n").expect("write fixture");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["branch", "feature"]);
    git(&source, ["tag", "lightweight"]);
    git_with_env(&source, ["tag", "-a", "annotated", "-m", "tag message"]);
    git(
        dir.path(),
        [
            "clone",
            "--bare",
            source.to_str().expect("source path"),
            "git.git",
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            "--bare",
            source.to_str().expect("source path"),
            "zmin.git",
        ],
    );
    let git_repo = dir.path().join("git.git");
    let zmin_repo = dir.path().join("zmin.git");

    git(&git_repo, ["update-server-info"]);
    run_zmin(&zmin_repo, ["update-server-info"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.join("info/refs")).expect("read zmin info refs"),
        fs::read_to_string(git_repo.join("info/refs")).expect("read git info refs")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.join("objects/info/packs")).expect("read zmin packs info"),
        fs::read_to_string(git_repo.join("objects/info/packs")).expect("read git packs info")
    );

    git(&git_repo, ["repack", "-adq"]);
    git(&zmin_repo, ["repack", "-adq"]);
    git(&git_repo, ["update-server-info", "-f"]);
    run_zmin(&zmin_repo, ["update-server-info", "-f"]);
    assert_eq!(
        fs::read_to_string(zmin_repo.join("info/refs")).expect("read packed zmin info refs"),
        fs::read_to_string(git_repo.join("info/refs")).expect("read packed git info refs")
    );
    assert_eq!(
        fs::read_to_string(zmin_repo.join("objects/info/packs"))
            .expect("read packed zmin packs info"),
        fs::read_to_string(git_repo.join("objects/info/packs"))
            .expect("read packed git packs info")
    );
}

#[test]
fn count_objects_matches_stock_git_for_loose_and_packed_objects() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    assert_eq!(
        run_zmin(repo.path(), ["count-objects"]),
        git(repo.path(), ["count-objects"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-H"]),
        git(repo.path(), ["count-objects", "-H"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-vH"]),
        git(repo.path(), ["count-objects", "-vH"])
    );

    git(repo.path(), ["repack", "-adq"]);
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-vH"]),
        git(repo.path(), ["count-objects", "-vH"])
    );
}

#[test]
fn count_objects_in_pack_counts_pack_index_entries_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write first");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    git(repo.path(), ["repack", "-adq"]);

    let saved_pack_dir = repo.path().join("saved-pack");
    fs::create_dir_all(&saved_pack_dir).expect("create saved pack dir");
    for entry in fs::read_dir(repo.path().join(".git/objects/pack")).expect("read pack dir") {
        let path = entry.expect("pack entry").path();
        if path.is_file() {
            fs::copy(
                &path,
                saved_pack_dir.join(path.file_name().expect("pack file name")),
            )
            .expect("save pack file");
        }
    }

    fs::write(repo.path().join("b.txt"), b"two\n").expect("write second");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    git(repo.path(), ["repack", "-adq"]);
    for entry in fs::read_dir(&saved_pack_dir).expect("read saved pack dir") {
        let path = entry.expect("saved pack entry").path();
        fs::copy(
            &path,
            repo.path()
                .join(".git/objects/pack")
                .join(path.file_name().expect("saved pack file name")),
        )
        .expect("restore saved pack file");
    }

    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
}

#[test]
fn count_objects_prune_packable_counts_loose_objects_once_with_duplicate_packs() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"one\n").expect("write first");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "first"]);
    let blob = git(repo.path(), ["rev-parse", "HEAD:a.txt"]);
    let loose_path = repo
        .path()
        .join(".git/objects")
        .join(&blob[..2])
        .join(&blob[2..]);
    let loose_copy = fs::read(&loose_path).expect("read loose blob");
    git(repo.path(), ["repack", "-adq"]);

    let saved_pack_dir = repo.path().join("saved-pack");
    fs::create_dir_all(&saved_pack_dir).expect("create saved pack dir");
    for entry in fs::read_dir(repo.path().join(".git/objects/pack")).expect("read pack dir") {
        let path = entry.expect("pack entry").path();
        if path.is_file() {
            fs::copy(
                &path,
                saved_pack_dir.join(path.file_name().expect("pack file name")),
            )
            .expect("save pack file");
        }
    }

    fs::write(repo.path().join("b.txt"), b"two\n").expect("write second");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "second"]);
    git(repo.path(), ["repack", "-adq"]);
    for entry in fs::read_dir(&saved_pack_dir).expect("read saved pack dir") {
        let path = entry.expect("saved pack entry").path();
        fs::copy(
            &path,
            repo.path()
                .join(".git/objects/pack")
                .join(path.file_name().expect("saved pack file name")),
        )
        .expect("restore saved pack file");
    }
    fs::create_dir_all(loose_path.parent().expect("loose parent")).expect("recreate loose parent");
    fs::write(&loose_path, loose_copy).expect("restore loose blob");

    assert_eq!(
        run_zmin(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
}

#[test]
fn write_tree_and_commit_tree_match_stock_git_objects() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    fs::write(repo.path().join("src/lib.rs"), b"pub fn fixture() {}\n").expect("write source");
    git(repo.path(), ["add", "-A"]);

    let git_tree = git(repo.path(), ["write-tree"]);
    let zmin_tree = run_zmin(repo.path(), ["write-tree"]);
    assert_eq!(zmin_tree, git_tree);
    assert_eq!(
        run_zmin(repo.path(), ["write-tree", "--prefix=src"]),
        git(repo.path(), ["write-tree", "--prefix=src"])
    );

    let git_root = git_with_env(repo.path(), ["commit-tree", &git_tree, "-m", "root"]);
    let zmin_root = run_zmin_with_env(repo.path(), ["commit-tree", &git_tree, "-m", "root"]);
    assert_eq!(zmin_root, git_root);
    assert_eq!(
        run_zmin(repo.path(), ["cat-file", "-p", &zmin_root]),
        git(repo.path(), ["cat-file", "-p", &git_root])
    );

    fs::write(repo.path().join("a.txt"), b"second\n").expect("modify fixture");
    git(repo.path(), ["add", "-A"]);
    let tree = git(repo.path(), ["write-tree"]);
    let git_child = git_with_env(
        repo.path(),
        ["commit-tree", &tree, "-p", &git_root, "-m", "child"],
    );
    let zmin_child = run_zmin_with_env(
        repo.path(),
        ["commit-tree", &tree, "-p", &zmin_root, "-m", "child"],
    );
    assert_eq!(zmin_child, git_child);

    let git_dedup = git_with_env(
        repo.path(),
        [
            "commit-tree",
            &tree,
            "-p",
            &git_root,
            "-p",
            &git_root,
            "-m",
            "dedup",
        ],
    );
    let zmin_dedup = run_zmin_with_env(
        repo.path(),
        [
            "commit-tree",
            &tree,
            "-p",
            &zmin_root,
            "-p",
            &zmin_root,
            "-m",
            "dedup",
        ],
    );
    assert_eq!(zmin_dedup, git_dedup);
}

#[test]
fn write_tree_reuses_index_cache_without_returning_missing_tree_object() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    fs::create_dir_all(repo.path().join("src")).expect("create src");
    fs::write(repo.path().join("src/lib.rs"), b"pub fn fixture() {}\n").expect("write source");
    git(repo.path(), ["add", "-A"]);

    let first_tree = run_zmin(repo.path(), ["write-tree"]);
    assert_eq!(first_tree, git(repo.path(), ["write-tree"]));

    let cache_path = repo.path().join(".git/zmin/write-tree-cache-v1");
    assert!(cache_path.is_file(), "write-tree cache file should exist");

    let loose_tree = loose_object_path(repo.path(), &first_tree);
    fs::remove_file(&loose_tree).expect("remove loose tree object");
    assert!(
        !loose_tree.exists(),
        "test setup should remove the original loose tree object"
    );

    let second_tree = run_zmin(repo.path(), ["write-tree"]);
    assert_eq!(second_tree, first_tree);
    assert!(
        loose_tree.exists(),
        "cached write-tree should rebuild a missing tree object instead of returning a stale id"
    );
}

#[test]
fn show_pretty_raw_for_commit_tree_records_tree_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        configure_identity(repo);
        fs::write(repo.join("a.txt"), b"hello\n").expect("write fixture");
        git(repo, ["add", "-A"]);
    }
    let git_tree = git(git_repo.path(), ["write-tree"]);
    let zmin_tree = git(zmin_repo.path(), ["write-tree"]);
    assert_eq!(zmin_tree, git_tree);

    let git_commit = git_with_stdin(git_repo.path(), ["commit-tree", &git_tree], "NO\n");
    let zmin_commit = run_zmin_with_stdin(zmin_repo.path(), ["commit-tree", &zmin_tree], "NO\n");
    let git_raw = git(
        git_repo.path(),
        ["show", "--pretty=raw", "--no-patch", &git_commit],
    );
    let zmin_raw = run_zmin(
        zmin_repo.path(),
        ["show", "--pretty=raw", "--no-patch", &zmin_commit],
    );

    assert_eq!(
        zmin_raw
            .lines()
            .find(|line| line.starts_with("tree "))
            .map(str::to_owned),
        git_raw
            .lines()
            .find(|line| line.starts_with("tree "))
            .map(str::to_owned)
    );
    assert!(
        zmin_raw
            .lines()
            .take_while(|line| !line.starts_with("author "))
            .any(|line| line == format!("tree {zmin_tree}")),
        "show --pretty=raw should print tree before author: {zmin_raw}"
    );
}

#[test]
fn read_tree_matches_stock_git_for_tree_empty_and_prefix() {
    let git_repo = two_commit_repo();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    let tree = git(git_repo.path(), ["rev-parse", "HEAD^{tree}"]);

    git(git_repo.path(), ["read-tree", &tree]);
    run_zmin(zmin_repo.path(), ["read-tree", &tree]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["write-tree"]),
        git(git_repo.path(), ["write-tree"])
    );

    git(git_repo.path(), ["read-tree", "--empty"]);
    run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
    git(git_repo.path(), ["read-tree", "-m", &tree]);
    run_zmin(zmin_repo.path(), ["read-tree", "-m", &tree]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );

    git(git_repo.path(), ["read-tree", "--empty"]);
    run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );

    git(git_repo.path(), ["read-tree", "--prefix=import/", &tree]);
    run_zmin(zmin_repo.path(), ["read-tree", "--prefix=import/", &tree]);
    assert_eq!(
        run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );
    assert_eq!(
        run_zmin(zmin_repo.path(), ["write-tree"]),
        git(git_repo.path(), ["write-tree"])
    );
}

#[test]
fn read_and_write_tree_reject_null_sha_entries_like_stock_git() {
    let repo = git_init();
    let tree = git_with_stdin(
        repo.path(),
        ["mktree"],
        "160000 commit 0000000000000000000000000000000000000000\tbroken\n",
    );
    let args = ["read-tree", tree.trim()];
    assert_eq!(
        run_zmin_failure_output(repo.path(), &args),
        git_failure_output(repo.path(), &args)
    );

    let allow = [("GIT_ALLOW_NULL_SHA1", "1")];
    assert_eq!(
        command_output_with_env(zmin_bin(), repo.path(), &args, &allow, "zmin"),
        command_output_with_env("git", repo.path(), &args, &allow, "git")
    );
    assert_eq!(
        run_zmin_failure_output(repo.path(), &["write-tree"]),
        command_failure_output_with_env("git", repo.path(), &["write-tree"], &[], "git")
    );
}

#[test]
fn read_tree_partial_clone_prefetches_missing_blobs_in_one_batch() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("one"), b"foo\n").expect("write one");
    fs::write(source.join("two"), b"bar\n").expect("write two");
    git(&source, ["add", "one", "two"]);
    git_with_env(&source, ["commit", "-m", "initial"]);
    git(&source, ["config", "uploadpack.allowfilter", "true"]);
    git(&source, ["config", "uploadpack.allowanysha1inwant", "true"]);
    let tree = git(&source, ["rev-parse", "HEAD^{tree}"]);
    let one = git(&source, ["rev-parse", "HEAD:one"]);
    let two = git(&source, ["rev-parse", "HEAD:two"]);
    let client = dir.path().join("client.git");
    let source_url = format!("file://{}", source.display());
    run_zmin_args(
        dir.path(),
        &[
            "clone",
            "--bare",
            "--filter=blob:none",
            &source_url,
            client.to_str().expect("client path"),
        ],
    );

    for object_id in [&one, &two] {
        assert_ne!(
            run_zmin_status(&client, ["--no-lazy-fetch", "cat-file", "-e", object_id]),
            0,
            "partial clone unexpectedly contains {object_id}"
        );
    }

    let trace = dir.path().join("packet.trace");
    command_output_with_env(
        zmin_bin(),
        &client,
        &["read-tree", &tree, &tree],
        &[("GIT_TRACE_PACKET", trace.to_str().expect("trace path"))],
        "zmin read-tree partial clone",
    );

    let done_lines = fs::read_to_string(trace)
        .expect("read packet trace")
        .lines()
        .filter(|line| line.contains("fetch> done"))
        .count();
    assert_eq!(done_lines, 1);
    for object_id in [&one, &two] {
        assert_eq!(
            run_zmin_status(&client, ["--no-lazy-fetch", "cat-file", "-e", object_id]),
            0,
            "read-tree did not prefetch {object_id}"
        );
    }
}

#[test]
fn read_tree_documented_option_forms_match_stock_git() {
    let tree_repo = two_commit_repo();
    let tree = git(tree_repo.path(), ["rev-parse", "HEAD~1^{tree}"]);

    for args in [
        ["read-tree", "--empty", "--prefix=import/"].as_slice(),
        ["read-tree", "--empty", &tree].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(tree_repo.path());
        let zmin_repo = clone_repo_fixture(tree_repo.path());
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), args),
            git_failure_output(git_repo.path(), args),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
    }

    for args in [
        ["read-tree", "--empty"].as_slice(),
        ["read-tree", "--prefix", "import/", &tree].as_slice(),
        ["read-tree", "--prefix=import", &tree].as_slice(),
        ["read-tree", "-m", "-m", &tree].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(tree_repo.path());
        let zmin_repo = clone_repo_fixture(tree_repo.path());
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin"),
            command_any_output("git", git_repo.path(), args, "git"),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["write-tree"]),
            git(git_repo.path(), ["write-tree"]),
            "args: {args:?}"
        );
    }

    for args in [
        ["read-tree", "--dry-run", &tree].as_slice(),
        ["read-tree", "--dry-run", "--dry-run", &tree].as_slice(),
        ["read-tree", "-n", &tree].as_slice(),
        ["read-tree", "-n", "-n", &tree].as_slice(),
        ["read-tree", "--dry-run", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "--dry-run", &tree].as_slice(),
        ["read-tree", "-n", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "-n", &tree].as_slice(),
        ["read-tree", "-v", &tree].as_slice(),
        ["read-tree", "-v", "-v", &tree].as_slice(),
        ["read-tree", "-v", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "-v", &tree].as_slice(),
        ["read-tree", "--trivial", &tree].as_slice(),
        ["read-tree", "--trivial", "--trivial", &tree].as_slice(),
        ["read-tree", "--trivial", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "--trivial", &tree].as_slice(),
        ["read-tree", "--aggressive", &tree].as_slice(),
        ["read-tree", "--aggressive", "--aggressive", &tree].as_slice(),
        ["read-tree", "--aggressive", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "--aggressive", &tree].as_slice(),
        ["read-tree", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "-q", &tree].as_slice(),
        ["read-tree", "-q", "--quiet", &tree].as_slice(),
        ["read-tree", "-q", &tree].as_slice(),
        ["read-tree", "-q", "-q", &tree].as_slice(),
        ["read-tree", "--reset", &tree].as_slice(),
        ["read-tree", "--reset", "--reset", &tree].as_slice(),
        ["read-tree", "--reset", "--index-output=alt.index", &tree].as_slice(),
        ["read-tree", "--index-output=alt.index", "--reset", &tree].as_slice(),
        ["read-tree", "--no-sparse-checkout", &tree].as_slice(),
        [
            "read-tree",
            "--no-sparse-checkout",
            "--no-sparse-checkout",
            &tree,
        ]
        .as_slice(),
        ["read-tree", "--no-sparse-checkout", "--quiet", &tree].as_slice(),
        ["read-tree", "--quiet", "--no-sparse-checkout", &tree].as_slice(),
        ["read-tree", "--recurse-submodules", &tree].as_slice(),
        [
            "read-tree",
            "--recurse-submodules",
            "--recurse-submodules",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "--recurse-submodules",
            "--no-recurse-submodules",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "--recurse-submodules",
            "--no-recurse-submodules",
            "--recurse-submodules",
            &tree,
        ]
        .as_slice(),
        ["read-tree", "--no-recurse-submodules", &tree].as_slice(),
        [
            "read-tree",
            "--no-recurse-submodules",
            "--no-recurse-submodules",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "--no-recurse-submodules",
            "--recurse-submodules",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "--no-recurse-submodules",
            "--recurse-submodules",
            "--no-recurse-submodules",
            &tree,
        ]
        .as_slice(),
        ["read-tree", "-i", "--prefix=import/", &tree].as_slice(),
        ["read-tree", "-i", "-i", "--prefix=import/", &tree].as_slice(),
        ["read-tree", "-i", "--reset", &tree].as_slice(),
        ["read-tree", "-i", "-i", "--reset", &tree].as_slice(),
        [
            "read-tree",
            "-i",
            "--prefix=import/",
            "--index-output=alt.index",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "-i",
            "--prefix=import/",
            "--index-output",
            "alt.index",
            &tree,
        ]
        .as_slice(),
        [
            "read-tree",
            "-i",
            "--prefix=import/",
            "--index-output=first.index",
            "--index-output=alt.index",
            &tree,
        ]
        .as_slice(),
    ] {
        let git_repo = clone_repo_fixture(tree_repo.path());
        let zmin_repo = clone_repo_fixture(tree_repo.path());
        git(git_repo.path(), ["read-tree", "--empty"]);
        run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin"),
            command_any_output("git", git_repo.path(), args, "git"),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["write-tree"]),
            git(git_repo.path(), ["write-tree"]),
            "args: {args:?}"
        );
        if args.contains(&"--index-output=alt.index") {
            assert_eq!(
                command_output_with_env(
                    "git",
                    zmin_repo.path(),
                    &["write-tree"],
                    &[("GIT_INDEX_FILE", "alt.index")],
                    "git write-tree zmin alt index",
                )
                .1,
                command_output_with_env(
                    "git",
                    git_repo.path(),
                    &["write-tree"],
                    &[("GIT_INDEX_FILE", "alt.index")],
                    "git write-tree git alt index",
                )
                .1,
                "args: {args:?}"
            );
        }
    }

    for args in [
        ["read-tree", "-m", "-u", &tree].as_slice(),
        ["read-tree", "-m", "-u", "-u", &tree].as_slice(),
        ["read-tree", "--reset", "-u", &tree].as_slice(),
        ["read-tree", "-u", "-u", "--reset", &tree].as_slice(),
        ["read-tree", "--reset", "-u", "-u", &tree].as_slice(),
        ["read-tree", "-u", "--quiet", "--reset", &tree].as_slice(),
        ["read-tree", "--quiet", "-u", "--reset", &tree].as_slice(),
        ["read-tree", "-u", "--trivial", "--reset", &tree].as_slice(),
        ["read-tree", "-u", "--aggressive", "--reset", &tree].as_slice(),
        ["read-tree", "--prefix=import/", "-u", &tree].as_slice(),
        ["read-tree", "-u", "-u", "--prefix=import/", &tree].as_slice(),
        ["read-tree", "--prefix=import/", "-u", "-u", &tree].as_slice(),
        ["read-tree", "-u", "--quiet", "--prefix=import/", &tree].as_slice(),
        ["read-tree", "--quiet", "-u", "--prefix=import/", &tree].as_slice(),
        ["read-tree", "-m", "--trivial", "-u", &tree].as_slice(),
        ["read-tree", "-m", "--aggressive", "-u", &tree].as_slice(),
    ] {
        let git_repo = clone_repo_fixture(tree_repo.path());
        let zmin_repo = clone_repo_fixture(tree_repo.path());
        fs::remove_file(git_repo.path().join("a.txt")).expect("remove git worktree a");
        let _ = fs::remove_file(git_repo.path().join("b.txt"));
        fs::remove_file(zmin_repo.path().join("a.txt")).expect("remove zmin worktree a");
        let _ = fs::remove_file(zmin_repo.path().join("b.txt"));
        assert_eq!(
            command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin"),
            command_any_output("/usr/bin/git", git_repo.path(), args, "git"),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["write-tree"]),
            git(git_repo.path(), ["write-tree"]),
            "args: {args:?}"
        );
        if args.contains(&"--prefix=import/") {
            assert_eq!(
                fs::read(zmin_repo.path().join("import/a.txt")).expect("read zmin imported file"),
                fs::read(git_repo.path().join("import/a.txt")).expect("read git imported file"),
                "args: {args:?}"
            );
        } else {
            assert_eq!(
                fs::read(zmin_repo.path().join("a.txt")).expect("read zmin worktree file"),
                fs::read(git_repo.path().join("a.txt")).expect("read git worktree file"),
                "args: {args:?}"
            );
        }
    }

    {
        for args in [
            ["read-tree", "--index-output=alt.index", &tree].as_slice(),
            [
                "read-tree",
                "-u",
                "--index-output=alt.index",
                "--reset",
                &tree,
            ]
            .as_slice(),
            [
                "read-tree",
                "--index-output=alt.index",
                "-u",
                "--reset",
                &tree,
            ]
            .as_slice(),
            [
                "read-tree",
                "-u",
                "--index-output=alt.index",
                "--prefix=import/",
                &tree,
            ]
            .as_slice(),
            [
                "read-tree",
                "--index-output=alt.index",
                "-u",
                "--prefix=import/",
                &tree,
            ]
            .as_slice(),
        ] {
            let git_repo = clone_repo_fixture(tree_repo.path());
            let zmin_repo = clone_repo_fixture(tree_repo.path());
            git(git_repo.path(), ["read-tree", "--empty"]);
            run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
            if args.contains(&"-u") {
                let _ = fs::remove_file(git_repo.path().join("a.txt"));
                let _ = fs::remove_file(git_repo.path().join("b.txt"));
                let _ = fs::remove_file(zmin_repo.path().join("a.txt"));
                let _ = fs::remove_file(zmin_repo.path().join("b.txt"));
            }
            assert_eq!(
                command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin"),
                command_any_output("git", git_repo.path(), args, "git"),
                "args: {args:?}"
            );
            assert_eq!(
                run_zmin(zmin_repo.path(), ["write-tree"]),
                git(git_repo.path(), ["write-tree"]),
                "args: {args:?}"
            );
            assert_eq!(
                command_output_with_env(
                    "git",
                    zmin_repo.path(),
                    &["ls-files", "-s"],
                    &[("GIT_INDEX_FILE", "alt.index")],
                    "git ls-files zmin alt index",
                )
                .1,
                command_output_with_env(
                    "git",
                    git_repo.path(),
                    &["ls-files", "-s"],
                    &[("GIT_INDEX_FILE", "alt.index")],
                    "git ls-files git alt index",
                )
                .1,
                "args: {args:?}"
            );
        }
    }

    {
        let git_repo = clone_repo_fixture(tree_repo.path());
        let zmin_repo = clone_repo_fixture(tree_repo.path());
        git(git_repo.path(), ["read-tree", "--empty"]);
        run_zmin(zmin_repo.path(), ["read-tree", "--empty"]);
        for args in [
            ["read-tree", "-i", &tree].as_slice(),
            ["read-tree", "-i", "--index-output=alt.index", &tree].as_slice(),
            ["read-tree", "-u", &tree].as_slice(),
            ["read-tree", "-i", "-u", "--prefix=import/", &tree].as_slice(),
        ] {
            assert_eq!(
                run_zmin_failure_output(zmin_repo.path(), args),
                git_failure_output(git_repo.path(), args),
                "args: {args:?}"
            );
            assert_eq!(
                run_zmin(zmin_repo.path(), ["write-tree"]),
                git(git_repo.path(), ["write-tree"]),
                "args: {args:?}"
            );
        }
    }
}

#[test]
fn read_tree_confusing_paths_respect_protect_hfs_and_ntfs_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = clone_repo_fixture(git_repo.path());
    configure_identity(git_repo.path());
    configure_identity(zmin_repo.path());

    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join("file"), b"content\n").expect("write file");
        git(repo, ["add", "file"]);
        git_with_env(repo, ["commit", "-m", "base"]);
        git(repo, ["config", "core.protectHFS", "true"]);
        git(repo, ["config", "core.protectNTFS", "true"]);
    }

    let blob = git(git_repo.path(), ["rev-parse", "HEAD:file"]);
    let tree = git(git_repo.path(), ["rev-parse", "HEAD^{tree}"]);
    let blob_bytes = hex_to_bytes(&blob);
    let tree_bytes = hex_to_bytes(&tree);

    for (path, mode, object_bytes) in [
        (".", "100644", blob_bytes.as_slice()),
        ("..", "100644", blob_bytes.as_slice()),
        (".git", "100644", blob_bytes.as_slice()),
        (".GIT", "100644", blob_bytes.as_slice()),
        ("\u{200c}.Git", "100644", blob_bytes.as_slice()),
        (".gI\u{200c}T", "100644", blob_bytes.as_slice()),
        (".GiT\u{200c}", "100644", blob_bytes.as_slice()),
        ("git~1", "100644", blob_bytes.as_slice()),
        (".git. ", "100644", blob_bytes.as_slice()),
        (".\\\\.GIT\\\\foobar", "100644", blob_bytes.as_slice()),
        (".git\\\\foobar", "100644", blob_bytes.as_slice()),
        (".git...:alternate-stream", "100644", blob_bytes.as_slice()),
        (".", "040000", tree_bytes.as_slice()),
        ("..", "040000", tree_bytes.as_slice()),
        (".git", "040000", tree_bytes.as_slice()),
        (".GIT", "040000", tree_bytes.as_slice()),
        ("\u{200c}.Git", "040000", tree_bytes.as_slice()),
        (".gI\u{200c}T", "040000", tree_bytes.as_slice()),
        (".GiT\u{200c}", "040000", tree_bytes.as_slice()),
        ("git~1", "040000", tree_bytes.as_slice()),
        (".git. ", "040000", tree_bytes.as_slice()),
        (".\\\\.GIT\\\\foobar", "040000", tree_bytes.as_slice()),
        (".git\\\\foobar", "040000", tree_bytes.as_slice()),
        (".git...:alternate-stream", "040000", tree_bytes.as_slice()),
    ] {
        let mut raw = Vec::new();
        raw.extend_from_slice(mode.as_bytes());
        raw.push(b' ');
        raw.extend_from_slice(path.as_bytes());
        raw.push(0);
        raw.extend_from_slice(object_bytes);

        let git_tree = git_with_stdin_bytes(
            git_repo.path(),
            ["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
            &raw,
        );
        let zmin_tree = git_with_stdin_bytes(
            zmin_repo.path(),
            ["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
            &raw,
        );

        assert_ne!(
            git_status(git_repo.path(), ["read-tree", &git_tree]),
            0,
            "stock git unexpectedly accepted path {path:?} mode {mode}"
        );
        assert_ne!(
            run_zmin_status(zmin_repo.path(), ["read-tree", &zmin_tree]),
            0,
            "zmin unexpectedly accepted path {path:?} mode {mode}"
        );
    }
}

#[test]
fn read_tree_update_worktree_collision_and_super_prefix_match_stock_git() {
    let setup_repo = |repo: &std::path::Path| {
        let _ = fs::remove_file(repo.join(".git/index"));
        fs::write(repo.join("a"), b"").expect("write fixture a");
        git(repo, ["update-index", "--add", "a"]);
        let tree_m = git(repo, ["write-tree"]);
        fs::remove_file(repo.join("a")).expect("remove fixture a");
        git(repo, ["update-index", "--remove", "a"]);
        fs::create_dir(repo.join("a")).expect("create fixture a dir");
        fs::write(repo.join("a/b"), b"").expect("write fixture a/b");
        let tree_h = git(repo, ["write-tree"]);
        (tree_h, tree_m)
    };

    for with_super_prefix in [false, true] {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let (git_tree_h, git_tree_m) = setup_repo(git_repo.path());
        let (zmin_tree_h, zmin_tree_m) = setup_repo(zmin_repo.path());
        assert_eq!(git_tree_h, zmin_tree_h, "treeH fixture diverged");
        assert_eq!(git_tree_m, zmin_tree_m, "treeM fixture diverged");

        let owned_args = if with_super_prefix {
            vec![
                "read-tree".to_owned(),
                "--super-prefix".to_owned(),
                "fictional/".to_owned(),
                "-u".to_owned(),
                "-m".to_owned(),
                git_tree_h.clone(),
                git_tree_m.clone(),
            ]
        } else {
            vec![
                "read-tree".to_owned(),
                "-n".to_owned(),
                "-u".to_owned(),
                "-m".to_owned(),
                git_tree_h.clone(),
                git_tree_m.clone(),
            ]
        };
        let args = owned_args.iter().map(String::as_str).collect::<Vec<_>>();

        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), &args),
            git_failure_output(git_repo.path(), &args),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
        assert!(
            zmin_repo.path().join("a/b").is_file(),
            "zmin should preserve untracked a/b for args: {args:?}"
        );
        assert!(
            git_repo.path().join("a/b").is_file(),
            "git should preserve untracked a/b for args: {args:?}"
        );
    }
}

#[test]
fn read_tree_three_way_current_index_matrix_matches_stock_git() {
    let setup_repo = |repo: &std::path::Path| {
        let _ = fs::remove_file(repo.join(".git/index"));
        fs::write(repo.join("f"), b"base\n").expect("write base");
        git(repo, ["update-index", "--add", "f"]);
        let tree_o = git(repo, ["write-tree"]);
        let tree_a = tree_o.clone();
        fs::write(repo.join("f"), b"theirs\n").expect("write theirs");
        git(repo, ["update-index", "f"]);
        let tree_b = git(repo, ["write-tree"]);
        (tree_o, tree_a, tree_b)
    };

    {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let (git_tree_o, git_tree_a, git_tree_b) = setup_repo(git_repo.path());
        let (zmin_tree_o, zmin_tree_a, zmin_tree_b) = setup_repo(zmin_repo.path());
        assert_eq!(
            (git_tree_o.clone(), git_tree_a.clone(), git_tree_b.clone()),
            (
                zmin_tree_o.clone(),
                zmin_tree_a.clone(),
                zmin_tree_b.clone()
            )
        );

        for repo in [git_repo.path(), zmin_repo.path()] {
            let _ = fs::remove_file(repo.join(".git/index"));
            fs::write(repo.join("f"), b"theirs\n").expect("write current b");
            git(repo, ["update-index", "--add", "f"]);
            fs::write(repo.join("f"), b"theirs\nextra\n").expect("dirty worktree");
        }

        let args = [
            "read-tree",
            "-m",
            git_tree_o.as_str(),
            git_tree_a.as_str(),
            git_tree_b.as_str(),
        ];
        run_zmin(zmin_repo.path(), args);
        git(git_repo.path(), args);
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"])
        );
        assert_eq!(
            fs::read(zmin_repo.path().join("f")).expect("read zmin worktree"),
            fs::read(git_repo.path().join("f")).expect("read git worktree")
        );
    }

    {
        let git_repo = git_init();
        let zmin_repo = git_init();
        let (git_tree_o, git_tree_a, git_tree_b) = setup_repo(git_repo.path());
        let (zmin_tree_o, zmin_tree_a, zmin_tree_b) = setup_repo(zmin_repo.path());
        assert_eq!(
            (git_tree_o.clone(), git_tree_a.clone(), git_tree_b.clone()),
            (
                zmin_tree_o.clone(),
                zmin_tree_a.clone(),
                zmin_tree_b.clone()
            )
        );

        for repo in [git_repo.path(), zmin_repo.path()] {
            let _ = fs::remove_file(repo.join(".git/index"));
            fs::write(repo.join("f"), b"other\n").expect("write mismatched current");
            git(repo, ["update-index", "--add", "f"]);
        }

        let args = [
            "read-tree",
            "-m",
            git_tree_o.as_str(),
            git_tree_a.as_str(),
            git_tree_b.as_str(),
        ];
        assert_eq!(
            run_zmin_failure_output(zmin_repo.path(), &args),
            git_failure_output(git_repo.path(), &args)
        );
    }
}

#[test]
fn read_tree_three_way_directory_file_conflicts_match_stock_git() {
    let setup_repo = |repo: &std::path::Path| {
        let make_tree = |name: &str, items: &[&str]| {
            let _ = fs::remove_file(repo.join(".git/index"));
            let _ = fs::remove_file(repo.join(".git/index.lock"));
            git(repo, ["clean", "-d", "-f", "-f", "-q", "-x"]);
            for item in items {
                let path = item.split(':').next().expect("path component");
                if let Some(parent) = std::path::Path::new(path).parent() {
                    fs::create_dir_all(repo.join(parent)).expect("create parent dirs");
                }
                fs::write(repo.join(path), format!("{item}\n")).expect("write tree entry");
                git(repo, ["update-index", "--add", path]);
            }
            git(repo, ["tag", name, &git(repo, ["write-tree"])]);
        };

        make_tree("O-000", &["a/b-2/c/d", "a/b/c/d", "a/x"]);
        make_tree("A-000", &["a/b-2/c/d", "a/b/c/d", "a/x"]);
        make_tree("A-001", &["a/b-2/c/d", "a/b/c/d", "a/b/c/e", "a/x"]);
        make_tree("B-000", &["a/b-2/c/d", "a/b", "a/x"]);
        make_tree("O-010", &["t-0", "t/1", "t/2", "t=3"]);
        make_tree("A-010", &["t-0", "t", "t=3"]);
        make_tree("B-010", &["t/1:", "t=3:"]);
    };

    let set_tree = |repo: &std::path::Path, tree: &str| {
        let _ = fs::remove_file(repo.join(".git/index"));
        let _ = fs::remove_file(repo.join(".git/index.lock"));
        git(repo, ["clean", "-d", "-f", "-f", "-q", "-x"]);
        git(repo, ["read-tree", tree]);
        git(repo, ["checkout-index", "-f", "-q", "-u", "-a"]);
        git(repo, ["update-index", "--refresh"]);
    };

    let git_repo = git_init();
    let zmin_repo = git_init();
    setup_repo(git_repo.path());
    setup_repo(zmin_repo.path());

    for (tree, args) in [
        (
            "A-000",
            vec!["read-tree", "-m", "-u", "O-000", "A-000", "B-000"],
        ),
        (
            "A-001",
            vec!["read-tree", "-m", "-u", "O-000", "A-001", "B-000"],
        ),
        (
            "A-010",
            vec!["read-tree", "-m", "-u", "O-010", "A-010", "B-010"],
        ),
    ] {
        set_tree(git_repo.path(), tree);
        set_tree(zmin_repo.path(), tree);
        run_zmin_args(zmin_repo.path(), &args);
        let _ = command_any_output("git", git_repo.path(), &args, "read-tree");
        assert_eq!(
            run_zmin(zmin_repo.path(), ["ls-files", "-s"]),
            git(git_repo.path(), ["ls-files", "-s"]),
            "args: {args:?}"
        );
    }
}

#[test]
fn mktree_matches_stock_git_for_text_nul_and_batch_input() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"a\n").expect("write a");
    fs::write(repo.path().join("b.txt"), b"b\n").expect("write b");
    let a = git(repo.path(), ["hash-object", "-w", "a.txt"]);
    let b = git(repo.path(), ["hash-object", "-w", "b.txt"]);

    let input = format!("100644 blob {b}\tb.txt\n100644 blob {a}\ta.txt\n");
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktree"], &input),
        git_with_stdin(repo.path(), ["mktree"], &input)
    );

    let nul_input = format!("100644 blob {a}\ta.txt\0");
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktree", "-z"], &nul_input),
        git_with_stdin(repo.path(), ["mktree", "-z"], &nul_input)
    );

    let batch_input = format!(
        "100644 blob {a}\ta.txt\n\n100644 blob {b}\tb.txt\n160000 commit 1111111111111111111111111111111111111111\tsub\n"
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktree", "--batch"], &batch_input),
        git_with_stdin(repo.path(), ["mktree", "--batch"], &batch_input)
    );
}

#[test]
fn mktag_matches_stock_git_for_valid_tag_object() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"a\n").expect("write a");
    let blob = git(repo.path(), ["hash-object", "-w", "a.txt"]);
    let input = format!(
        "object {blob}\ntype blob\ntag v1\ntagger Bench <bench@example.test> 1700000000 +0000\n\ntag message\n"
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktag"], &input),
        git_with_stdin(repo.path(), ["mktag"], &input)
    );
    assert_eq!(
        run_zmin_with_stdin(repo.path(), ["mktag", "--strict"], &input),
        git_with_stdin(repo.path(), ["mktag", "--strict"], &input)
    );
}
