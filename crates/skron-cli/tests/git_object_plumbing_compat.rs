mod common;

use std::{collections::BTreeSet, fs};

use tempfile::TempDir;

use common::{
    clone_repo_fixture, configure_identity, git, git_args, git_init, git_status, git_with_env,
    git_with_stdin, git_with_stdin_bytes, run_skron, run_skron_args, run_skron_status,
    run_skron_with_env, run_skron_with_stdin, run_skron_with_stdin_bytes,
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

#[test]
fn hash_object_and_cat_file_match_stock_git() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");

    let git_id = git(repo.path(), ["hash-object", "-w", "a.txt"]);
    let skron_id = run_skron(repo.path(), ["hash-object", "-w", "a.txt"]);
    assert_eq!(skron_id, git_id);
    assert_eq!(
        run_skron_with_stdin(repo.path(), ["hash-object", "--stdin", "a.txt"], "stdin\n"),
        git_with_stdin(repo.path(), ["hash-object", "--stdin", "a.txt"], "stdin\n")
    );
    let large_stdin = vec![b'x'; 300 * 1024];
    let git_large_id =
        git_with_stdin_bytes(repo.path(), ["hash-object", "-w", "--stdin"], &large_stdin);
    let skron_large_id =
        run_skron_with_stdin_bytes(repo.path(), ["hash-object", "-w", "--stdin"], &large_stdin);
    assert_eq!(skron_large_id, git_large_id);
    assert_eq!(
        run_skron(repo.path(), ["cat-file", "-s", &skron_large_id]),
        git(repo.path(), ["cat-file", "-s", &git_large_id])
    );

    assert_eq!(
        run_skron(repo.path(), ["cat-file", "-t", &git_id]),
        git(repo.path(), ["cat-file", "-t", &git_id])
    );
    assert_eq!(
        run_skron(repo.path(), ["cat-file", "-s", &git_id]),
        git(repo.path(), ["cat-file", "-s", &git_id])
    );
    assert_eq!(
        run_skron(
            repo.path(),
            ["cat-file", "--batch-all-objects", "--batch-check"]
        ),
        git(
            repo.path(),
            ["cat-file", "--batch-all-objects", "--batch-check"]
        )
    );
    let skron_unordered = run_skron(
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
        skron_unordered.lines().collect::<BTreeSet<_>>(),
        git_unordered.lines().collect::<BTreeSet<_>>()
    );
    assert_eq!(
        run_skron(
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
        run_skron_with_stdin(
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
        run_skron_with_stdin(
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
        run_skron_status(repo.path(), ["cat-file", "-e", &git_id]),
        git_status(repo.path(), ["cat-file", "-e", &git_id])
    );

    git(repo.path(), ["repack", "-adq"]);
    assert_eq!(
        run_skron(repo.path(), ["cat-file", "-t", &git_id]),
        git(repo.path(), ["cat-file", "-t", &git_id])
    );
    assert_eq!(
        run_skron(repo.path(), ["cat-file", "-s", &git_id]),
        git(repo.path(), ["cat-file", "-s", &git_id])
    );
    assert_eq!(
        run_skron_with_stdin(
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
        run_skron_status(repo.path(), ["cat-file", "-e", &git_id]),
        git_status(repo.path(), ["cat-file", "-e", &git_id])
    );
}

#[test]
fn index_stage_object_paths_match_stock_git() {
    let repo = git_init();
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "a.txt"]);

    for objectish in [":a.txt", ":0:a.txt"] {
        assert_eq!(
            run_skron(repo.path(), ["rev-parse", objectish]),
            git(repo.path(), ["rev-parse", objectish])
        );
        assert_eq!(
            run_skron(repo.path(), ["cat-file", "-p", objectish]),
            git(repo.path(), ["cat-file", "-p", objectish])
        );
    }
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
            run_skron_args(repo.path(), args),
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
        ["ls-tree", "-r", "--name-only", "HEAD"].as_slice(),
    ] {
        assert_eq!(
            run_skron_args(repo.path(), args),
            git_args(repo.path(), args),
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

    let skron_path = run_skron(repo.path(), ["unpack-file", &blob]);
    assert!(skron_path.starts_with(".merge_file_"));
    assert_eq!(
        fs::read(repo.path().join(&skron_path)).expect("read skron unpacked file"),
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
        run_skron_status(repo.path(), ["unpack-file", &commit]),
        git_status(repo.path(), ["unpack-file", &commit])
    );
    assert_eq!(
        run_skron_status(repo.path(), ["unpack-file", "deadbeef"]),
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
        run_skron_with_stdin_bytes(repo.path(), ["show-index"], &idx),
        git_with_stdin_bytes(repo.path(), ["show-index"], &idx)
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
            "skron.git",
        ],
    );
    let git_repo = dir.path().join("git.git");
    let skron_repo = dir.path().join("skron.git");

    git(&git_repo, ["update-server-info"]);
    run_skron(&skron_repo, ["update-server-info"]);
    assert_eq!(
        fs::read_to_string(skron_repo.join("info/refs")).expect("read skron info refs"),
        fs::read_to_string(git_repo.join("info/refs")).expect("read git info refs")
    );
    assert_eq!(
        fs::read_to_string(skron_repo.join("objects/info/packs")).expect("read skron packs info"),
        fs::read_to_string(git_repo.join("objects/info/packs")).expect("read git packs info")
    );

    git(&git_repo, ["repack", "-adq"]);
    git(&skron_repo, ["repack", "-adq"]);
    git(&git_repo, ["update-server-info", "-f"]);
    run_skron(&skron_repo, ["update-server-info", "-f"]);
    assert_eq!(
        fs::read_to_string(skron_repo.join("info/refs")).expect("read packed skron info refs"),
        fs::read_to_string(git_repo.join("info/refs")).expect("read packed git info refs")
    );
    assert_eq!(
        fs::read_to_string(skron_repo.join("objects/info/packs"))
            .expect("read packed skron packs info"),
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
        run_skron(repo.path(), ["count-objects"]),
        git(repo.path(), ["count-objects"])
    );
    assert_eq!(
        run_skron(repo.path(), ["count-objects", "-H"]),
        git(repo.path(), ["count-objects", "-H"])
    );
    assert_eq!(
        run_skron(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
    assert_eq!(
        run_skron(repo.path(), ["count-objects", "-vH"]),
        git(repo.path(), ["count-objects", "-vH"])
    );

    git(repo.path(), ["repack", "-adq"]);
    assert_eq!(
        run_skron(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
    assert_eq!(
        run_skron(repo.path(), ["count-objects", "-vH"]),
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
        run_skron(repo.path(), ["count-objects", "-v"]),
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
        run_skron(repo.path(), ["count-objects", "-v"]),
        git(repo.path(), ["count-objects", "-v"])
    );
}

#[test]
fn write_tree_and_commit_tree_match_stock_git_objects() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write fixture");
    git(repo.path(), ["add", "-A"]);

    let git_tree = git(repo.path(), ["write-tree"]);
    let skron_tree = run_skron(repo.path(), ["write-tree"]);
    assert_eq!(skron_tree, git_tree);

    let git_root = git_with_env(repo.path(), ["commit-tree", &git_tree, "-m", "root"]);
    let skron_root = run_skron_with_env(repo.path(), ["commit-tree", &git_tree, "-m", "root"]);
    assert_eq!(skron_root, git_root);
    assert_eq!(
        run_skron(repo.path(), ["cat-file", "-p", &skron_root]),
        git(repo.path(), ["cat-file", "-p", &git_root])
    );

    fs::write(repo.path().join("a.txt"), b"second\n").expect("modify fixture");
    git(repo.path(), ["add", "-A"]);
    let tree = git(repo.path(), ["write-tree"]);
    let git_child = git_with_env(
        repo.path(),
        ["commit-tree", &tree, "-p", &git_root, "-m", "child"],
    );
    let skron_child = run_skron_with_env(
        repo.path(),
        ["commit-tree", &tree, "-p", &skron_root, "-m", "child"],
    );
    assert_eq!(skron_child, git_child);
}

#[test]
fn read_tree_matches_stock_git_for_tree_empty_and_prefix() {
    let git_repo = two_commit_repo();
    let skron_repo = clone_repo_fixture(git_repo.path());
    let tree = git(git_repo.path(), ["rev-parse", "HEAD^{tree}"]);

    git(git_repo.path(), ["read-tree", &tree]);
    run_skron(skron_repo.path(), ["read-tree", &tree]);
    assert_eq!(
        run_skron(skron_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["write-tree"]),
        git(git_repo.path(), ["write-tree"])
    );

    git(git_repo.path(), ["read-tree", "--empty"]);
    run_skron(skron_repo.path(), ["read-tree", "--empty"]);
    assert_eq!(
        run_skron(skron_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );

    git(git_repo.path(), ["read-tree", "--prefix=import/", &tree]);
    run_skron(skron_repo.path(), ["read-tree", "--prefix=import/", &tree]);
    assert_eq!(
        run_skron(skron_repo.path(), ["ls-files", "-s"]),
        git(git_repo.path(), ["ls-files", "-s"])
    );
    assert_eq!(
        run_skron(skron_repo.path(), ["write-tree"]),
        git(git_repo.path(), ["write-tree"])
    );
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
        run_skron_with_stdin(repo.path(), ["mktree"], &input),
        git_with_stdin(repo.path(), ["mktree"], &input)
    );

    let nul_input = format!("100644 blob {a}\ta.txt\0");
    assert_eq!(
        run_skron_with_stdin(repo.path(), ["mktree", "-z"], &nul_input),
        git_with_stdin(repo.path(), ["mktree", "-z"], &nul_input)
    );

    let batch_input = format!(
        "100644 blob {a}\ta.txt\n\n100644 blob {b}\tb.txt\n160000 commit 1111111111111111111111111111111111111111\tsub\n"
    );
    assert_eq!(
        run_skron_with_stdin(repo.path(), ["mktree", "--batch"], &batch_input),
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
        run_skron_with_stdin(repo.path(), ["mktag"], &input),
        git_with_stdin(repo.path(), ["mktag"], &input)
    );
    assert_eq!(
        run_skron_with_stdin(repo.path(), ["mktag", "--strict"], &input),
        git_with_stdin(repo.path(), ["mktag", "--strict"], &input)
    );
}
