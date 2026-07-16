mod common;

use std::fs;
use std::process::Command;

use tempfile::TempDir;

fn zmin_bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_zmin").unwrap_or(env!("CARGO_BIN_EXE_zmin"))
}

#[test]
fn clone_unsupported_remote_helper_failure_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");

    assert_eq!(
        command_output(
            zmin_bin(),
            dir.path(),
            &["clone", "zminproto://example/repo", "dst"],
            "zmin",
        ),
        command_output(
            "git",
            dir.path(),
            &["clone", "zminproto://example/repo", "dst"],
            "git",
        )
    );
}

#[test]
fn clone_ref_format_files_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = create_clone_source(dir.path(), "source");

    git(
        dir.path(),
        [
            "clone",
            "--ref-format=files",
            source.to_str().expect("source path"),
            "git-files",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--ref-format=files",
            source.to_str().expect("source path"),
            "zmin-files",
        ],
    );
    assert_eq!(
        run_zmin(&dir.path().join("zmin-files"), ["rev-parse", "HEAD"]),
        command_output(
            "git",
            &dir.path().join("git-files"),
            &["rev-parse", "HEAD"],
            "git"
        )
        .1
    );
}

#[test]
fn clone_relative_dot_records_fetchable_remote_url_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = create_clone_source(dir.path(), "source");

    git(&source, ["clone", ".", "git-dot"]);
    run_zmin(&source, ["clone", ".", "zmin-dot"]);

    let git_clone = source.join("git-dot");
    let zmin_clone = source.join("zmin-dot");
    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "remote.origin.url"]),
        git(&git_clone, ["config", "--get", "remote.origin.url"])
    );

    fs::write(source.join("README.md"), b"updated\n").expect("update source");
    git(&source, ["commit", "-am", "update"]);
    git(&git_clone, ["fetch"]);
    run_zmin(&zmin_clone, ["fetch"]);
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "refs/remotes/origin/main"]),
        git(&git_clone, ["rev-parse", "refs/remotes/origin/main"])
    );
}

#[test]
fn t1050_clone_file_transport_keeps_a_verifiable_pack_like_stock_git() {
    let root = TempDir::new().expect("temp dir");
    let source = create_clone_source(root.path(), "source");
    let source_url = format!("file://{}", source.display());
    let git_root = root.path().join("git-root");
    let zmin_root = root.path().join("zmin-root");
    fs::create_dir(&git_root).expect("create git clone root");
    fs::create_dir(&zmin_root).expect("create zmin clone root");

    let git_output = command_output(
        common::stock_git_bin()
            .to_str()
            .expect("stock git path utf8"),
        &git_root,
        &["clone", &source_url, "client"],
        "git file clone",
    );
    let zmin_output = command_output(
        zmin_bin(),
        &zmin_root,
        &["clone", &source_url, "client"],
        "zmin file clone",
    );
    assert_eq!(zmin_output, git_output);

    let git_packs = clone_pack_files(&git_root.join("client/.git"));
    let zmin_packs = clone_pack_files(&zmin_root.join("client/.git"));
    assert_eq!(zmin_packs.len(), git_packs.len());
    assert_eq!(zmin_packs.len(), 1);
    for pack in git_packs.iter().chain(&zmin_packs) {
        let output = Command::new(common::stock_git_bin())
            .arg("--git-dir=non-existent")
            .args(["index-pack", "--object-format=sha1", "--strict", "--verify"])
            .arg(pack)
            .current_dir(root.path())
            .output()
            .expect("verify clone pack");
        assert!(
            output.status.success(),
            "index-pack failed for {}: {}",
            pack.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn clone_pack_files(git_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut packs = fs::read_dir(git_dir.join("objects/pack"))
        .expect("read clone pack directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "pack")
        })
        .collect::<Vec<_>>();
    packs.sort();
    packs
}

#[test]
fn clone_url_insteadof_rewrites_transport_but_preserves_stored_remote_url() {
    let dir = TempDir::new().expect("temp dir");
    let source = create_clone_source(dir.path(), "source");
    let git_config = dir.path().join("gitconfig");
    fs::write(
        &git_config,
        format!(
            "[url \"file://{}/\"]\n\tinsteadOf = https://example.test/\n",
            dir.path().display()
        ),
    )
    .expect("write gitconfig");

    let typed_url = "https://example.test/source";
    let git_clone = command_output_with_env(
        common::stock_git_bin().to_str().expect("stock git utf8"),
        dir.path(),
        &["clone", typed_url, "git-alias"],
        &[(
            "GIT_CONFIG_GLOBAL",
            git_config.to_str().expect("gitconfig path utf8"),
        )],
        "git clone with insteadOf",
    );
    assert_eq!(git_clone.0, 0, "stock clone failed: {:?}", git_clone);

    let zmin_clone = command_output_with_env(
        zmin_bin(),
        dir.path(),
        &["clone", typed_url, "zmin-alias"],
        &[(
            "GIT_CONFIG_GLOBAL",
            git_config.to_str().expect("gitconfig path utf8"),
        )],
        "zmin clone with insteadOf",
    );
    assert_eq!(zmin_clone.0, 0, "zmin clone failed: {:?}", zmin_clone);

    let git_repo = dir.path().join("git-alias");
    let zmin_repo = dir.path().join("zmin-alias");
    assert_eq!(
        git_args_with_env(
            &git_repo,
            &["config", "--get", "remote.origin.url"],
            &[(
                "GIT_CONFIG_GLOBAL",
                git_config.to_str().expect("gitconfig path utf8"),
            )],
        ),
        git_args_with_env(
            &zmin_repo,
            &["config", "--get", "remote.origin.url"],
            &[(
                "GIT_CONFIG_GLOBAL",
                git_config.to_str().expect("gitconfig path utf8"),
            )],
        )
    );
    assert_eq!(
        git_args_with_env(
            &zmin_repo,
            &["config", "--get", "remote.origin.url"],
            &[(
                "GIT_CONFIG_GLOBAL",
                git_config.to_str().expect("gitconfig path utf8"),
            )],
        ),
        typed_url
    );
    assert_eq!(
        git_args_with_env(
            &git_repo,
            &["remote", "get-url", "origin"],
            &[(
                "GIT_CONFIG_GLOBAL",
                git_config.to_str().expect("gitconfig path utf8"),
            )],
        ),
        git_args_with_env(
            &zmin_repo,
            &["remote", "get-url", "origin"],
            &[(
                "GIT_CONFIG_GLOBAL",
                git_config.to_str().expect("gitconfig path utf8"),
            )],
        )
    );

    fs::write(source.join("README.md"), b"aliased update\n").expect("update source");
    git(&source, ["commit", "-am", "aliased update"]);
    command_output_with_env(
        common::stock_git_bin().to_str().expect("stock git utf8"),
        &git_repo,
        &["fetch", "origin"],
        &[(
            "GIT_CONFIG_GLOBAL",
            git_config.to_str().expect("gitconfig path utf8"),
        )],
        "git fetch with insteadOf",
    );
    command_output_with_env(
        zmin_bin(),
        &zmin_repo,
        &["fetch", "origin"],
        &[(
            "GIT_CONFIG_GLOBAL",
            git_config.to_str().expect("gitconfig path utf8"),
        )],
        "zmin fetch with insteadOf",
    );
    assert_eq!(
        git(&zmin_repo, ["rev-parse", "refs/remotes/origin/main"]),
        git(&git_repo, ["rev-parse", "refs/remotes/origin/main"])
    );
}

#[test]
fn clone_empty_template_bare_from_dot_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = create_clone_source(dir.path(), "source");

    git(
        &source,
        ["clone", "--template=", "--bare", ".", "git-bare-dot.git"],
    );
    run_zmin(
        &source,
        ["clone", "--template=", "--bare", ".", "zmin-bare-dot.git"],
    );

    let git_clone = source.join("git-bare-dot.git");
    let zmin_clone = source.join("zmin-bare-dot.git");
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "--is-bare-repository"]),
        git(&git_clone, ["rev-parse", "--is-bare-repository"])
    );
    assert_eq!(
        git(&zmin_clone, ["symbolic-ref", "HEAD"]),
        git(&git_clone, ["symbolic-ref", "HEAD"])
    );
    assert_eq!(
        git(&zmin_clone, ["show-ref"]),
        git(&git_clone, ["show-ref"])
    );
    assert_eq!(
        git_clone.join("info").exists(),
        zmin_clone.join("info").exists()
    );
    assert_eq!(
        git_clone.join("hooks").exists(),
        zmin_clone.join("hooks").exists()
    );
    assert_eq!(
        git_clone.join("description").exists(),
        zmin_clone.join("description").exists()
    );
}

#[test]
fn clone_instant_local_repo_marks_worktree_first_without_changing_git_state() {
    let dir = TempDir::new().expect("temp dir");
    let source = create_clone_source(dir.path(), "source");
    fs::write(source.join("README.md"), b"second\n").expect("update source");
    git(&source, ["commit", "-am", "second"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            "--instant",
            source.to_str().expect("source path"),
            "zmin-instant",
        ],
    );
    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-clone"],
    );

    let zmin_clone = dir.path().join("zmin-instant");
    let git_clone = dir.path().join("git-clone");
    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD"]),
        git(&git_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(zmin_clone.join("README.md")).expect("zmin readme"),
        fs::read_to_string(git_clone.join("README.md")).expect("git readme")
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["status", "--porcelain=v1", "--branch"]),
        git(&git_clone, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn clone_instant_local_repo_fetch_and_pull_remain_canonical_git_operations() {
    let dir = TempDir::new().expect("temp dir");
    let source = create_clone_source(dir.path(), "source");
    git(&source, ["config", "core.autocrlf", "false"]);

    run_zmin(
        dir.path(),
        [
            "clone",
            "--instant",
            source.to_str().expect("source path"),
            "zmin-instant",
        ],
    );
    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-clone"],
    );

    let zmin_clone = dir.path().join("zmin-instant");
    let git_clone = dir.path().join("git-clone");
    git(&zmin_clone, ["config", "core.autocrlf", "false"]);
    git(&git_clone, ["config", "core.autocrlf", "false"]);

    fs::write(source.join("README.md"), b"pulled\n").expect("update readme");
    fs::write(source.join("crlf.txt"), b"line one\r\nline two\r\n").expect("write crlf");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "pull update"]);
    let expected_head = git(&source, ["rev-parse", "HEAD"]);

    run_zmin(&zmin_clone, ["fetch", "origin"]);
    git(&git_clone, ["fetch", "origin"]);
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "refs/remotes/origin/main"]),
        git(&git_clone, ["rev-parse", "refs/remotes/origin/main"])
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );

    run_zmin(&zmin_clone, ["pull", "--ff-only"]);
    git(&git_clone, ["pull", "--ff-only"]);

    assert_eq!(git(&zmin_clone, ["rev-parse", "HEAD"]), expected_head);
    assert_eq!(
        git(&zmin_clone, ["rev-parse", "HEAD^{tree}"]),
        git(&git_clone, ["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        fs::read(zmin_clone.join("crlf.txt")).expect("zmin crlf"),
        fs::read(git_clone.join("crlf.txt")).expect("git crlf")
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["config", "--get", "zmin.worktreeFirst"]),
        "true"
    );
    assert_eq!(
        run_zmin(&zmin_clone, ["status", "--porcelain=v1", "--branch"]),
        git(&git_clone, ["status", "--porcelain=v1", "--branch"])
    );
}

#[test]
fn clone_worktree_first_rejects_non_worktree_or_remote_modes() {
    let dir = TempDir::new().expect("temp dir");
    let source = create_clone_source(dir.path(), "source");

    let bare = command_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--worktree-first",
            "--bare",
            source.to_str().expect("source path"),
            "zmin-bare",
        ],
        "zmin",
    );
    assert_eq!(bare.0, 129);
    assert!(bare.2.contains("requires a working tree"));

    let no_checkout = command_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--instant",
            "--no-checkout",
            source.to_str().expect("source path"),
            "zmin-no-checkout",
        ],
        "zmin",
    );
    assert_eq!(no_checkout.0, 129);
    assert!(
        no_checkout
            .2
            .contains("cannot be combined with --no-checkout")
    );

    let background_without_instant = command_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--background-fetch",
            source.to_str().expect("source path"),
            "zmin-background-standard",
        ],
        "zmin",
    );
    assert_eq!(background_without_instant.0, 129);
    assert!(
        background_without_instant
            .2
            .contains("requires --worktree-first or --instant")
    );

    let background_local = command_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--instant",
            "--background-fetch",
            source.to_str().expect("source path"),
            "zmin-background-local",
        ],
        "zmin",
    );
    assert_eq!(background_local.0, 129);
    assert!(
        background_local
            .2
            .contains("requires an HTTP, SSH, or git daemon remote")
    );

    let demand_without_instant = command_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--demand-hydrate",
            source.to_str().expect("source path"),
            "zmin-demand-standard",
        ],
        "zmin",
    );
    assert_eq!(demand_without_instant.0, 129);
    assert!(
        demand_without_instant
            .2
            .contains("requires --worktree-first or --instant")
    );

    let demand_local = command_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--instant",
            "--demand-hydrate",
            source.to_str().expect("source path"),
            "zmin-demand-local",
        ],
        "zmin",
    );
    assert_eq!(demand_local.0, 129);
    assert!(
        demand_local
            .2
            .contains("requires an HTTP, SSH, or git daemon remote")
    );
}

#[cfg(unix)]
#[test]
fn clone_rejects_symlinked_local_object_store_like_stock_git() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().expect("temp dir");
    let source = create_clone_source(dir.path(), "source");
    let real_objects = dir.path().join("objects-real");
    fs::rename(source.join(".git/objects"), &real_objects).expect("move objects");
    symlink(&real_objects, source.join(".git/objects")).expect("symlink objects");

    let git_root = command_output(
        "git",
        dir.path(),
        &["clone", source.to_str().expect("source path"), "git-root"],
        "git",
    );
    let zmin_root = command_output(
        zmin_bin(),
        dir.path(),
        &["clone", source.to_str().expect("source path"), "zmin-root"],
        "zmin",
    );
    assert_eq!(zmin_root.0, git_root.0);
    assert!(zmin_root.2.contains("refusing to clone with --local"));

    fs::remove_file(source.join(".git/objects")).expect("remove objects symlink");
    fs::rename(&real_objects, source.join(".git/objects")).expect("restore objects");
    let loose_object = first_loose_object(&source.join(".git/objects"));
    fs::remove_file(&loose_object).expect("remove loose object");
    symlink("/etc/passwd", &loose_object).expect("symlink loose object");

    let git_entry = command_output(
        "git",
        dir.path(),
        &["clone", source.to_str().expect("source path"), "git-entry"],
        "git",
    );
    let zmin_entry = command_output(
        zmin_bin(),
        dir.path(),
        &["clone", source.to_str().expect("source path"), "zmin-entry"],
        "zmin",
    );
    assert_eq!(zmin_entry.0, git_entry.0);
    assert!(zmin_entry.2.contains("refusing to clone with --local"));
}

#[cfg(unix)]
#[test]
fn fetch_rejects_symlinked_destination_object_store() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().expect("temp dir");
    let source = create_clone_source(dir.path(), "source");
    run_zmin(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "zmin-clone"],
    );
    let clone = dir.path().join("zmin-clone");
    let loose_object = first_loose_object(&source.join(".git/objects"));
    let object_dir = loose_object
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .expect("loose object dir");
    let destination_object_dir = clone.join(".git/objects").join(object_dir);
    fs::remove_dir_all(&destination_object_dir).expect("remove destination object dir");
    symlink(dir.path(), &destination_object_dir).expect("symlink destination object dir");

    let output = command_output(zmin_bin(), &clone, &["fetch", "origin"], "zmin");

    assert_eq!(output.0, 128);
    assert!(output.2.contains("destination object path"));
}

#[cfg(unix)]
#[test]
fn clone_skips_symlink_directory_case_collision_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    if !case_insensitive_filesystem(dir.path()) {
        return;
    }

    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    let empty_oid = git_with_stdin(&source, &["hash-object", "-w", "--stdin"], "");
    let symlink_oid = git_with_stdin(&source, &["hash-object", "-w", "--stdin"], "target-dir");
    git_with_stdin(
        &source,
        &["update-index", "--index-info"],
        &format!("100644 blob {empty_oid}\tA/x\n120000 blob {symlink_oid}\ta\n"),
    );
    git_with_env(&source, ["commit", "-m", "case collision"]);

    let git_clone = command_output(
        "git",
        dir.path(),
        &["clone", source.to_str().expect("source path"), "git-clone"],
        "git",
    );
    let zmin_clone = command_output(
        zmin_bin(),
        dir.path(),
        &["clone", source.to_str().expect("source path"), "zmin-clone"],
        "zmin",
    );

    assert_eq!(zmin_clone.0, git_clone.0, "{}", zmin_clone.2);
    assert!(dir.path().join("zmin-clone/A/x").is_file());
    assert!(dir.path().join("zmin-clone/a").is_dir());
}

#[test]
fn clone_local_repo_matches_stock_git_state() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["tag", "v1"]);
    git(&source, ["switch", "-c", "feature"]);
    fs::write(source.join("feature.txt"), b"feature\n").expect("write feature");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature"]);
    git(&source, ["switch", "main"]);
    let reference = dir.path().join("reference");
    git(
        dir.path(),
        [
            "init",
            "-b",
            "main",
            reference.to_str().expect("reference path"),
        ],
    );
    configure_identity(&reference);
    fs::write(reference.join("reference.txt"), b"reference\n").expect("write reference");
    git(&reference, ["add", "-A"]);
    git_with_env(&reference, ["commit", "-m", "reference"]);

    git(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "git-clone"],
    );
    run_zmin(
        dir.path(),
        ["clone", source.to_str().expect("source path"), "zmin-clone"],
    );
    let git_clone = dir.path().join("git-clone");
    let zmin_clone = dir.path().join("zmin-clone");

    assert_eq!(
        git(zmin_clone.as_path(), ["rev-parse", "HEAD"]),
        git(git_clone.as_path(), ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(zmin_clone.as_path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_clone.as_path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        run_zmin(zmin_clone.as_path(), ["remote", "-v"]),
        git(git_clone.as_path(), ["remote", "-v"])
    );
    assert_eq!(
        run_zmin(zmin_clone.as_path(), ["branch", "-r"]),
        git(git_clone.as_path(), ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(
            zmin_clone.as_path(),
            ["status", "--porcelain=v1", "--branch"]
        ),
        git(
            git_clone.as_path(),
            ["status", "--porcelain=v1", "--branch"]
        )
    );

    assert_eq!(
        command_output(
            zmin_bin(),
            dir.path(),
            &[
                "clone",
                "--quiet",
                source.to_str().expect("source path"),
                "zmin-quiet-clone",
            ],
            "zmin"
        ),
        command_output(
            "git",
            dir.path(),
            &[
                "clone",
                "--quiet",
                source.to_str().expect("source path"),
                "git-quiet-clone",
            ],
            "git"
        )
    );

    for (flag, git_dir_name, zmin_dir_name) in [
        ("--local", "git-local-clone", "zmin-local-clone"),
        ("--no-local", "git-no-local-clone", "zmin-no-local-clone"),
        (
            "--no-hardlinks",
            "git-no-hardlinks-clone",
            "zmin-no-hardlinks-clone",
        ),
        ("--hardlinks", "git-hardlinks-clone", "zmin-hardlinks-clone"),
    ] {
        git(
            dir.path(),
            [
                "clone",
                flag,
                source.to_str().expect("source path"),
                git_dir_name,
            ],
        );
        run_zmin(
            dir.path(),
            [
                "clone",
                flag,
                source.to_str().expect("source path"),
                zmin_dir_name,
            ],
        );
        assert_eq!(
            run_zmin(
                &dir.path().join(zmin_dir_name),
                ["status", "--porcelain=v1", "--branch"]
            ),
            git(
                &dir.path().join(git_dir_name),
                ["status", "--porcelain=v1", "--branch"]
            ),
            "clone flag mismatch for {flag}"
        );
    }

    git(
        dir.path(),
        [
            "clone",
            "-c",
            "core.autocrlf=input",
            "-c",
            "clone.flag.without.value",
            "-c",
            "remote.origin.tagOpt=--no-tags",
            source.to_str().expect("source path"),
            "git-config-clone",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "-c",
            "core.autocrlf=input",
            "-c",
            "clone.flag.without.value",
            "-c",
            "remote.origin.tagOpt=--no-tags",
            source.to_str().expect("source path"),
            "zmin-config-clone",
        ],
    );
    let git_config_clone = dir.path().join("git-config-clone");
    let zmin_config_clone = dir.path().join("zmin-config-clone");
    for key in [
        "core.autocrlf",
        "clone.flag.without.value",
        "remote.origin.tagOpt",
    ] {
        assert_eq!(
            run_zmin(&zmin_config_clone, ["config", "--get", key]),
            git(&git_config_clone, ["config", "--get", key]),
            "clone config mismatch for {key}"
        );
    }
    assert_eq!(
        run_zmin(&zmin_config_clone, ["status", "--porcelain=v1", "--branch"]),
        git(&git_config_clone, ["status", "--porcelain=v1", "--branch"])
    );

    let template = dir.path().join("template");
    fs::create_dir_all(template.join("hooks")).expect("create template hooks");
    fs::create_dir_all(template.join("info")).expect("create template info");
    fs::write(template.join("hooks/pre-commit"), b"#!/bin/sh\n").expect("write template hook");
    fs::write(template.join("info/exclude"), b"*.tmp\n").expect("write template exclude");
    fs::write(template.join("description"), b"template description\n")
        .expect("write template description");
    fs::write(
        template.join("config"),
        b"[custom]\n\tvalue = from-template\n",
    )
    .expect("write template config");
    git(
        dir.path(),
        [
            "clone",
            "--template",
            template.to_str().expect("template path"),
            source.to_str().expect("source path"),
            "git-template-clone",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--template",
            template.to_str().expect("template path"),
            source.to_str().expect("source path"),
            "zmin-template-clone",
        ],
    );
    let git_template_clone = dir.path().join("git-template-clone");
    let zmin_template_clone = dir.path().join("zmin-template-clone");
    for path in [
        ".git/hooks/pre-commit",
        ".git/info/exclude",
        ".git/description",
    ] {
        assert_eq!(
            fs::read_to_string(zmin_template_clone.join(path)).expect("zmin template file"),
            fs::read_to_string(git_template_clone.join(path)).expect("git template file"),
            "template file mismatch for {path}"
        );
    }
    assert_eq!(
        run_zmin(&zmin_template_clone, ["config", "--get", "custom.value"]),
        git(&git_template_clone, ["config", "--get", "custom.value"])
    );
    assert_eq!(
        run_zmin(
            &zmin_template_clone,
            ["status", "--porcelain=v1", "--branch"]
        ),
        git(
            &git_template_clone,
            ["status", "--porcelain=v1", "--branch"]
        )
    );
    for (args, git_name, zmin_name) in [
        (
            [
                "--no-template",
                "--template",
                template.to_str().expect("template path"),
            ]
            .as_slice(),
            "git-template-last",
            "zmin-template-last",
        ),
        (
            [
                "--template",
                template.to_str().expect("template path"),
                "--no-template",
            ]
            .as_slice(),
            "git-no-template-last",
            "zmin-no-template-last",
        ),
    ] {
        let mut git_clone_args = vec!["clone"];
        git_clone_args.extend_from_slice(args);
        git_clone_args.extend_from_slice(&[source.to_str().expect("source path"), git_name]);
        let mut zmin_clone_args = vec!["clone"];
        zmin_clone_args.extend_from_slice(args);
        zmin_clone_args.extend_from_slice(&[source.to_str().expect("source path"), zmin_name]);
        git_args(dir.path(), &git_clone_args);
        run_zmin_args(dir.path(), &zmin_clone_args);
        let git_clone = dir.path().join(git_name);
        let zmin_clone = dir.path().join(zmin_name);
        assert_eq!(
            command_output(
                zmin_bin(),
                &zmin_clone,
                &["config", "--get", "custom.value"],
                "zmin"
            ),
            command_output(
                "git",
                &git_clone,
                &["config", "--get", "custom.value"],
                "git"
            ),
            "template order config mismatch for {args:?}"
        );
        assert_eq!(
            zmin_clone.join(".git/hooks/pre-commit").exists(),
            git_clone.join(".git/hooks/pre-commit").exists(),
            "template order hook mismatch for {args:?}"
        );
    }

    git(
        dir.path(),
        [
            "clone",
            "-o",
            "upstream",
            source.to_str().expect("source path"),
            "git-origin-clone",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "-o",
            "upstream",
            source.to_str().expect("source path"),
            "zmin-origin-clone",
        ],
    );
    let zmin_origin_clone = dir.path().join("zmin-origin-clone");
    let git_origin_clone = dir.path().join("git-origin-clone");
    assert_eq!(
        run_zmin(&zmin_origin_clone, ["remote", "-v"]),
        git(&git_origin_clone, ["remote", "-v"])
    );
    assert_eq!(
        run_zmin(&zmin_origin_clone, ["branch", "-r"]),
        git(&git_origin_clone, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(
            &zmin_origin_clone,
            ["config", "--get", "branch.main.remote"]
        ),
        git(&git_origin_clone, ["config", "--get", "branch.main.remote"])
    );

    git(
        dir.path(),
        [
            "clone",
            "--no-tags",
            source.to_str().expect("source path"),
            "git-no-tags",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--no-tags",
            source.to_str().expect("source path"),
            "zmin-no-tags",
        ],
    );
    let zmin_no_tags = dir.path().join("zmin-no-tags");
    let git_no_tags = dir.path().join("git-no-tags");
    assert_eq!(
        command_output(zmin_bin(), &zmin_no_tags, &["show-ref", "--tags"], "zmin"),
        command_output("git", &git_no_tags, &["show-ref", "--tags"], "git")
    );
    assert_eq!(
        run_zmin(&zmin_no_tags, ["config", "--get", "remote.origin.tagOpt"]),
        git(&git_no_tags, ["config", "--get", "remote.origin.tagOpt"])
    );
    for (args, git_name, zmin_name) in [
        (["--tags"].as_slice(), "git-tags", "zmin-tags"),
        (
            ["--no-tags", "--tags"].as_slice(),
            "git-tags-last",
            "zmin-tags-last",
        ),
        (
            ["--tags", "--no-tags"].as_slice(),
            "git-no-tags-last",
            "zmin-no-tags-last",
        ),
    ] {
        let mut git_clone_args = vec!["clone"];
        git_clone_args.extend_from_slice(args);
        git_clone_args.extend_from_slice(&[source.to_str().expect("source path"), git_name]);
        let mut zmin_clone_args = vec!["clone"];
        zmin_clone_args.extend_from_slice(args);
        zmin_clone_args.extend_from_slice(&[source.to_str().expect("source path"), zmin_name]);
        git_args(dir.path(), &git_clone_args);
        run_zmin_args(dir.path(), &zmin_clone_args);
        let git_clone = dir.path().join(git_name);
        let zmin_clone = dir.path().join(zmin_name);
        assert_eq!(
            run_zmin(&zmin_clone, ["tag"]),
            git(&git_clone, ["tag"]),
            "tag list mismatch for {args:?}"
        );
        assert_eq!(
            command_output(
                zmin_bin(),
                &zmin_clone,
                &["config", "--get", "remote.origin.tagOpt"],
                "zmin"
            ),
            command_output(
                "git",
                &git_clone,
                &["config", "--get", "remote.origin.tagOpt"],
                "git"
            ),
            "tagOpt mismatch for {args:?}"
        );
    }

    git(
        dir.path(),
        [
            "clone",
            "--reference",
            reference.to_str().expect("reference path"),
            source.to_str().expect("source path"),
            "git-reference-clone",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--reference",
            reference.to_str().expect("reference path"),
            source.to_str().expect("source path"),
            "zmin-reference-clone",
        ],
    );
    let zmin_reference_clone = dir.path().join("zmin-reference-clone");
    let git_reference_clone = dir.path().join("git-reference-clone");
    assert_eq!(
        fs::read_to_string(zmin_reference_clone.join(".git/objects/info/alternates"))
            .expect("zmin alternates"),
        fs::read_to_string(git_reference_clone.join(".git/objects/info/alternates"))
            .expect("git alternates")
    );
    assert_eq!(
        run_zmin(
            &zmin_reference_clone,
            ["status", "--porcelain=v1", "--branch"]
        ),
        git(
            &git_reference_clone,
            ["status", "--porcelain=v1", "--branch"]
        )
    );

    git(
        dir.path(),
        [
            "clone",
            "--reference-if-able",
            reference.to_str().expect("reference path"),
            source.to_str().expect("source path"),
            "git-reference-if-able-clone",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--reference-if-able",
            reference.to_str().expect("reference path"),
            source.to_str().expect("source path"),
            "zmin-reference-if-able-clone",
        ],
    );
    let zmin_reference_if_able_clone = dir.path().join("zmin-reference-if-able-clone");
    let git_reference_if_able_clone = dir.path().join("git-reference-if-able-clone");
    assert_eq!(
        canonical_alternates(&zmin_reference_if_able_clone.join(".git/objects/info/alternates")),
        canonical_alternates(&git_reference_if_able_clone.join(".git/objects/info/alternates"))
    );
    assert_eq!(
        run_zmin(
            &zmin_reference_if_able_clone,
            ["status", "--porcelain=v1", "--branch"]
        ),
        git(
            &git_reference_if_able_clone,
            ["status", "--porcelain=v1", "--branch"]
        )
    );

    git(
        dir.path(),
        [
            "clone",
            "--reference-if-able",
            "missing-reference",
            source.to_str().expect("source path"),
            "git-missing-reference-if-able-clone",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--reference-if-able",
            "missing-reference",
            source.to_str().expect("source path"),
            "zmin-missing-reference-if-able-clone",
        ],
    );
    assert_eq!(
        dir.path()
            .join("zmin-missing-reference-if-able-clone/.git/objects/info/alternates")
            .exists(),
        dir.path()
            .join("git-missing-reference-if-able-clone/.git/objects/info/alternates")
            .exists()
    );

    git(
        dir.path(),
        [
            "clone",
            "--shared",
            source.to_str().expect("source path"),
            "git-shared-clone",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--shared",
            source.to_str().expect("source path"),
            "zmin-shared-clone",
        ],
    );
    let zmin_shared_clone = dir.path().join("zmin-shared-clone");
    let git_shared_clone = dir.path().join("git-shared-clone");
    assert_eq!(
        canonical_alternates(&zmin_shared_clone.join(".git/objects/info/alternates")),
        canonical_alternates(&git_shared_clone.join(".git/objects/info/alternates"))
    );
    assert_eq!(
        run_zmin(&zmin_shared_clone, ["status", "--porcelain=v1", "--branch"]),
        git(&git_shared_clone, ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        run_zmin(&zmin_shared_clone, ["cat-file", "-t", "HEAD"]),
        git(&git_shared_clone, ["cat-file", "-t", "HEAD"])
    );

    git(
        dir.path(),
        [
            "clone",
            "--reference",
            reference.to_str().expect("reference path"),
            "--dissociate",
            source.to_str().expect("source path"),
            "git-dissociate-clone",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--reference",
            reference.to_str().expect("reference path"),
            "--dissociate",
            source.to_str().expect("source path"),
            "zmin-dissociate-clone",
        ],
    );
    assert_eq!(
        dir.path()
            .join("zmin-dissociate-clone/.git/objects/info/alternates")
            .exists(),
        dir.path()
            .join("git-dissociate-clone/.git/objects/info/alternates")
            .exists()
    );

    git(
        dir.path(),
        [
            "clone",
            "--shared",
            "--dissociate",
            source.to_str().expect("source path"),
            "git-shared-dissociate-clone",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--shared",
            "--dissociate",
            source.to_str().expect("source path"),
            "zmin-shared-dissociate-clone",
        ],
    );
    assert_eq!(
        dir.path()
            .join("zmin-shared-dissociate-clone/.git/objects/info/alternates")
            .exists(),
        dir.path()
            .join("git-shared-dissociate-clone/.git/objects/info/alternates")
            .exists()
    );
    let zmin_shared_dissociate = dir.path().join("zmin-shared-dissociate-clone");
    let git_shared_dissociate = dir.path().join("git-shared-dissociate-clone");
    assert_eq!(
        run_zmin(&zmin_shared_dissociate, ["cat-file", "-t", "HEAD"]),
        git(&git_shared_dissociate, ["cat-file", "-t", "HEAD"])
    );

    git(
        dir.path(),
        [
            "clone",
            "--single-branch",
            source.to_str().expect("source path"),
            "git-single-branch",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--single-branch",
            source.to_str().expect("source path"),
            "zmin-single-branch",
        ],
    );
    let zmin_single_branch = dir.path().join("zmin-single-branch");
    let git_single_branch = dir.path().join("git-single-branch");
    assert_eq!(
        run_zmin(&zmin_single_branch, ["branch", "-r"]),
        git(&git_single_branch, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(
            &zmin_single_branch,
            ["config", "--get", "remote.origin.fetch"]
        ),
        git(
            &git_single_branch,
            ["config", "--get", "remote.origin.fetch"]
        )
    );

    git(
        dir.path(),
        [
            "clone",
            "--depth",
            "1",
            source.to_str().expect("source path"),
            "git-depth-clone",
        ],
    );
    assert_eq!(
        run_zmin_status(
            dir.path(),
            [
                "clone",
                "--depth",
                "1",
                source.to_str().expect("source path"),
                "zmin-depth-clone",
            ],
        ),
        0
    );
    let zmin_depth_clone = dir.path().join("zmin-depth-clone");
    let git_depth_clone = dir.path().join("git-depth-clone");
    assert_eq!(
        git(&zmin_depth_clone, ["rev-parse", "HEAD"]),
        git(&git_depth_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        run_zmin(&zmin_depth_clone, ["branch", "-r"]),
        git(&git_depth_clone, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(
            &zmin_depth_clone,
            ["config", "--get", "remote.origin.fetch"]
        ),
        git(&git_depth_clone, ["config", "--get", "remote.origin.fetch"])
    );

    run_zmin(
        dir.path(),
        [
            "clone",
            "--depth",
            "1",
            "--no-local",
            source.to_str().expect("source path"),
            "zmin-no-local-depth-clone",
        ],
    );
    let zmin_no_local_depth_clone = dir.path().join("zmin-no-local-depth-clone");
    assert_eq!(
        run_zmin(
            &zmin_no_local_depth_clone,
            ["rev-parse", "--is-shallow-repository"]
        ),
        "true"
    );

    let source_file_url = format!("file://{}", source.display());
    git(
        dir.path(),
        ["clone", "--depth", "1", &source_file_url, "git-file-clone"],
    );
    run_zmin(
        dir.path(),
        ["clone", "--depth", "1", &source_file_url, "zmin-file-clone"],
    );
    let zmin_file_clone = dir.path().join("zmin-file-clone");
    let git_file_clone = dir.path().join("git-file-clone");
    assert_eq!(
        git(&zmin_file_clone, ["rev-parse", "HEAD"]),
        git(&git_file_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        run_zmin(&zmin_file_clone, ["branch", "-r"]),
        git(&git_file_clone, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(&zmin_file_clone, ["log", "--oneline", "--all"]),
        git(&git_file_clone, ["log", "--oneline", "--all"])
    );
    assert_eq!(
        fs::read_to_string(zmin_file_clone.join(".git/shallow")).expect("zmin shallow"),
        fs::read_to_string(git_file_clone.join(".git/shallow")).expect("git shallow")
    );

    assert_eq!(
        command_output(
            zmin_bin(),
            dir.path(),
            &[
                "clone",
                "--reject-shallow",
                git_file_clone.to_str().expect("shallow source path"),
                "reject-shallow-target",
            ],
            "zmin"
        ),
        command_output(
            "git",
            dir.path(),
            &[
                "clone",
                "--reject-shallow",
                git_file_clone.to_str().expect("shallow source path"),
                "reject-shallow-target",
            ],
            "git"
        )
    );
    git(
        dir.path(),
        [
            "clone",
            "--no-reject-shallow",
            git_file_clone.to_str().expect("shallow source path"),
            "git-no-reject-shallow",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--no-reject-shallow",
            git_file_clone.to_str().expect("shallow source path"),
            "zmin-no-reject-shallow",
        ],
    );
    assert_eq!(
        run_zmin(
            &dir.path().join("zmin-no-reject-shallow"),
            ["status", "--porcelain=v1", "--branch"]
        ),
        git(
            &dir.path().join("git-no-reject-shallow"),
            ["status", "--porcelain=v1", "--branch"]
        )
    );
    for (index, args) in [
        (0, ["--reject-shallow", "--no-reject-shallow"].as_slice()),
        (1, ["--no-reject-shallow", "--reject-shallow"].as_slice()),
    ] {
        let git_cwd = dir.path().join(format!("git-reject-order-{index}"));
        let zmin_cwd = dir.path().join(format!("zmin-reject-order-{index}"));
        fs::create_dir(&git_cwd).expect("create git reject order dir");
        fs::create_dir(&zmin_cwd).expect("create zmin reject order dir");
        let mut git_args = vec!["clone"];
        git_args.extend_from_slice(args);
        git_args.extend_from_slice(&[
            git_file_clone.to_str().expect("shallow source path"),
            "target",
        ]);
        let mut zmin_args = vec!["clone"];
        zmin_args.extend_from_slice(args);
        zmin_args.extend_from_slice(&[
            git_file_clone.to_str().expect("shallow source path"),
            "target",
        ]);
        assert_eq!(
            command_output(zmin_bin(), &zmin_cwd, &zmin_args, "zmin"),
            command_output("git", &git_cwd, &git_args, "git"),
            "reject-shallow order mismatch for {args:?}"
        );
    }

    for (label, source_arg) in [
        (
            "path",
            git_file_clone
                .to_str()
                .expect("shallow source utf8")
                .to_owned(),
        ),
        ("file", format!("file://{}", git_file_clone.display())),
    ] {
        let git_cwd = dir.path().join(format!("git-shallow-local-{label}"));
        let zmin_cwd = dir.path().join(format!("zmin-shallow-local-{label}"));
        fs::create_dir(&git_cwd).expect("create git shallow cwd");
        fs::create_dir(&zmin_cwd).expect("create zmin shallow cwd");
        let git_output = command_output("git", &git_cwd, &["clone", &source_arg, "dst"], "git");
        let zmin_output = command_output(
            zmin_bin(),
            &zmin_cwd,
            &["clone", &source_arg, "dst"],
            "zmin",
        );
        assert_eq!(
            zmin_output, git_output,
            "shallow local clone mismatch for {label}"
        );
        assert_eq!(
            zmin_cwd.join("dst").exists(),
            git_cwd.join("dst").exists(),
            "destination existence mismatch for {label}"
        );
    }

    git(
        dir.path(),
        [
            "clone",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "git-branch-clone",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "zmin-branch-clone",
        ],
    );
    let zmin_branch_clone = dir.path().join("zmin-branch-clone");
    let git_branch_clone = dir.path().join("git-branch-clone");
    assert_eq!(
        run_zmin(&zmin_branch_clone, ["branch", "--show-current"]),
        git(&git_branch_clone, ["branch", "--show-current"])
    );
    assert_eq!(
        git(&zmin_branch_clone, ["cat-file", "-p", "HEAD:feature.txt"]),
        git(&git_branch_clone, ["cat-file", "-p", "HEAD:feature.txt"])
    );

    git(
        dir.path(),
        [
            "clone",
            "--single-branch",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "git-single-feature",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--single-branch",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "zmin-single-feature",
        ],
    );
    let zmin_single_feature = dir.path().join("zmin-single-feature");
    let git_single_feature = dir.path().join("git-single-feature");
    assert_eq!(
        run_zmin(&zmin_single_feature, ["branch", "-r"]),
        git(&git_single_feature, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(
            &zmin_single_feature,
            ["config", "--get", "remote.origin.fetch"]
        ),
        git(
            &git_single_feature,
            ["config", "--get", "remote.origin.fetch"]
        )
    );

    git(
        dir.path(),
        [
            "clone",
            "--single-branch",
            "-b",
            "v1",
            source.to_str().expect("source path"),
            "git-single-tag",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--single-branch",
            "-b",
            "v1",
            source.to_str().expect("source path"),
            "zmin-single-tag",
        ],
    );
    let zmin_single_tag = dir.path().join("zmin-single-tag");
    let git_single_tag = dir.path().join("git-single-tag");
    assert_eq!(
        run_zmin(&zmin_single_tag, ["rev-parse", "--abbrev-ref", "HEAD"]),
        git(&git_single_tag, ["rev-parse", "--abbrev-ref", "HEAD"])
    );
    assert_eq!(
        run_zmin(&zmin_single_tag, ["branch", "-r"]),
        git(&git_single_tag, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(&zmin_single_tag, ["config", "--get", "remote.origin.fetch"]),
        git(&git_single_tag, ["config", "--get", "remote.origin.fetch"])
    );
    assert_eq!(
        run_zmin(&zmin_single_tag, ["show-ref", "--tags"]),
        git(&git_single_tag, ["show-ref", "--tags"])
    );

    git(
        dir.path(),
        [
            "clone",
            "--no-checkout",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "git-no-checkout",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--no-checkout",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "zmin-no-checkout",
        ],
    );
    let zmin_no_checkout = dir.path().join("zmin-no-checkout");
    let git_no_checkout = dir.path().join("git-no-checkout");
    assert_eq!(
        run_zmin(&zmin_no_checkout, ["branch", "--show-current"]),
        git(&git_no_checkout, ["branch", "--show-current"])
    );
    assert_eq!(
        run_zmin(&zmin_no_checkout, ["status", "--porcelain=v1", "--branch"]),
        git(&git_no_checkout, ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read_dir(&zmin_no_checkout)
            .expect("read zmin no checkout")
            .filter(|entry| entry.as_ref().expect("zmin no checkout entry").file_name() != ".git")
            .count(),
        fs::read_dir(&git_no_checkout)
            .expect("read git no checkout")
            .filter(|entry| entry.as_ref().expect("git no checkout entry").file_name() != ".git")
            .count()
    );
    assert_eq!(
        zmin_no_checkout.join(".git/index").exists(),
        git_no_checkout.join(".git/index").exists()
    );

    git(
        dir.path(),
        [
            "clone",
            "--separate-git-dir",
            dir.path()
                .join("git-separate-meta.git")
                .to_str()
                .expect("git separate dir"),
            source.to_str().expect("source path"),
            "git-separate-work",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--separate-git-dir",
            dir.path()
                .join("zmin-separate-meta.git")
                .to_str()
                .expect("zmin separate dir"),
            source.to_str().expect("source path"),
            "zmin-separate-work",
        ],
    );
    let zmin_separate_work = dir.path().join("zmin-separate-work");
    let git_separate_work = dir.path().join("git-separate-work");
    let zmin_separate_meta = dir.path().join("zmin-separate-meta.git");
    let git_separate_meta = dir.path().join("git-separate-meta.git");
    assert!(zmin_separate_work.join(".git").is_file());
    assert!(git_separate_work.join(".git").is_file());
    assert_eq!(
        git(&zmin_separate_work, ["rev-parse", "HEAD"]),
        git(&git_separate_work, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        run_zmin(
            &zmin_separate_work,
            ["status", "--porcelain=v1", "--branch"]
        ),
        git(&git_separate_work, ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read_to_string(zmin_separate_meta.join("HEAD")).expect("zmin separate HEAD"),
        fs::read_to_string(git_separate_meta.join("HEAD")).expect("git separate HEAD")
    );

    git(
        dir.path(),
        [
            "clone",
            "--depth",
            "1",
            "-b",
            "feature",
            &source_file_url,
            "git-file-branch-clone",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--depth",
            "1",
            "-b",
            "feature",
            &source_file_url,
            "zmin-file-branch-clone",
        ],
    );
    let zmin_file_branch_clone = dir.path().join("zmin-file-branch-clone");
    let git_file_branch_clone = dir.path().join("git-file-branch-clone");
    assert_eq!(
        run_zmin(&zmin_file_branch_clone, ["branch", "-a"]),
        git(&git_file_branch_clone, ["branch", "-a"])
    );
    assert_eq!(
        run_zmin(&zmin_file_branch_clone, ["log", "--oneline", "--all"]),
        git(&git_file_branch_clone, ["log", "--oneline", "--all"])
    );
    assert_eq!(
        fs::read_to_string(zmin_file_branch_clone.join(".git/shallow")).expect("zmin shallow"),
        fs::read_to_string(git_file_branch_clone.join(".git/shallow")).expect("git shallow")
    );

    git(
        dir.path(),
        [
            "clone",
            "--depth",
            "1",
            "--no-single-branch",
            &source_file_url,
            "git-file-no-single-branch",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--depth",
            "1",
            "--no-single-branch",
            &source_file_url,
            "zmin-file-no-single-branch",
        ],
    );
    let zmin_file_no_single_branch = dir.path().join("zmin-file-no-single-branch");
    let git_file_no_single_branch = dir.path().join("git-file-no-single-branch");
    assert_eq!(
        run_zmin(&zmin_file_no_single_branch, ["branch", "-r"]),
        git(&git_file_no_single_branch, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(
            &zmin_file_no_single_branch,
            ["config", "--get", "remote.origin.fetch"]
        ),
        git(
            &git_file_no_single_branch,
            ["config", "--get", "remote.origin.fetch"]
        )
    );
    assert_eq!(
        run_zmin(&zmin_file_no_single_branch, ["log", "--oneline", "--all"]),
        git(&git_file_no_single_branch, ["log", "--oneline", "--all"])
    );
    assert_eq!(
        fs::read_to_string(zmin_file_no_single_branch.join(".git/shallow")).expect("zmin shallow"),
        fs::read_to_string(git_file_no_single_branch.join(".git/shallow")).expect("git shallow")
    );
    for (index, args) in [
        (0, ["--single-branch", "--no-single-branch"].as_slice()),
        (1, ["--no-single-branch", "--single-branch"].as_slice()),
        (
            2,
            ["--depth", "1", "--single-branch", "--no-single-branch"].as_slice(),
        ),
        (
            3,
            ["--depth", "1", "--no-single-branch", "--single-branch"].as_slice(),
        ),
    ] {
        let git_name = format!("git-single-order-{index}");
        let zmin_name = format!("zmin-single-order-{index}");
        let mut git_clone_args = vec!["clone"];
        git_clone_args.extend_from_slice(args);
        git_clone_args.extend_from_slice(&[&source_file_url, &git_name]);
        let mut zmin_clone_args = vec!["clone"];
        zmin_clone_args.extend_from_slice(args);
        zmin_clone_args.extend_from_slice(&[&source_file_url, &zmin_name]);
        git_args(dir.path(), &git_clone_args);
        run_zmin_args(dir.path(), &zmin_clone_args);

        let git_clone = dir.path().join(git_name);
        let zmin_clone = dir.path().join(zmin_name);
        assert_eq!(
            run_zmin(&zmin_clone, ["branch", "-r"]),
            git(&git_clone, ["branch", "-r"]),
            "single-branch order refs mismatch for {args:?}"
        );
        assert_eq!(
            run_zmin(&zmin_clone, ["config", "--get", "remote.origin.fetch"]),
            git(&git_clone, ["config", "--get", "remote.origin.fetch"]),
            "single-branch order fetch refspec mismatch for {args:?}"
        );
    }

    git(
        dir.path(),
        [
            "clone",
            "--bare",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "git-bare.git",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--bare",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "zmin-bare.git",
        ],
    );
    let zmin_bare = dir.path().join("zmin-bare.git");
    let git_bare = dir.path().join("git-bare.git");
    assert_eq!(
        git(&zmin_bare, ["rev-parse", "--is-bare-repository"]),
        git(&git_bare, ["rev-parse", "--is-bare-repository"])
    );
    assert_eq!(
        git(&zmin_bare, ["symbolic-ref", "HEAD"]),
        git(&git_bare, ["symbolic-ref", "HEAD"])
    );
    assert_eq!(git(&zmin_bare, ["show-ref"]), git(&git_bare, ["show-ref"]));

    git(
        dir.path(),
        [
            "clone",
            "--shared",
            "--bare",
            source.to_str().expect("source path"),
            "git-shared-bare.git",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--shared",
            "--bare",
            source.to_str().expect("source path"),
            "zmin-shared-bare.git",
        ],
    );
    let zmin_shared_bare = dir.path().join("zmin-shared-bare.git");
    let git_shared_bare = dir.path().join("git-shared-bare.git");
    assert_eq!(
        canonical_alternates(&zmin_shared_bare.join("objects/info/alternates")),
        canonical_alternates(&git_shared_bare.join("objects/info/alternates"))
    );
    assert_eq!(
        git(&zmin_shared_bare, ["show-ref"]),
        git(&git_shared_bare, ["show-ref"])
    );

    git(
        dir.path(),
        [
            "clone",
            "--bare",
            "--no-tags",
            source.to_str().expect("source path"),
            "git-bare-no-tags.git",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--bare",
            "--no-tags",
            source.to_str().expect("source path"),
            "zmin-bare-no-tags.git",
        ],
    );
    let zmin_bare_no_tags = dir.path().join("zmin-bare-no-tags.git");
    let git_bare_no_tags = dir.path().join("git-bare-no-tags.git");
    assert_eq!(
        command_output("git", &zmin_bare_no_tags, &["show-ref", "--tags"], "git"),
        command_output("git", &git_bare_no_tags, &["show-ref", "--tags"], "git")
    );
    assert_eq!(
        git(
            &zmin_bare_no_tags,
            ["config", "--get", "remote.origin.tagOpt"]
        ),
        git(
            &git_bare_no_tags,
            ["config", "--get", "remote.origin.tagOpt"]
        )
    );

    git(&source, ["update-ref", "refs/meta/custom", "HEAD"]);
    git(
        dir.path(),
        [
            "clone",
            "--mirror",
            source.to_str().expect("source path"),
            "git-mirror.git",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--mirror",
            source.to_str().expect("source path"),
            "zmin-mirror.git",
        ],
    );
    let zmin_mirror = dir.path().join("zmin-mirror.git");
    let git_mirror = dir.path().join("git-mirror.git");
    assert_eq!(
        git(&zmin_mirror, ["rev-parse", "--is-bare-repository"]),
        git(&git_mirror, ["rev-parse", "--is-bare-repository"])
    );
    assert_eq!(
        git(&zmin_mirror, ["symbolic-ref", "HEAD"]),
        git(&git_mirror, ["symbolic-ref", "HEAD"])
    );
    assert_eq!(
        git(&zmin_mirror, ["show-ref"]),
        git(&git_mirror, ["show-ref"])
    );
    assert_eq!(
        git(&zmin_mirror, ["config", "--get", "remote.origin.fetch"]),
        git(&git_mirror, ["config", "--get", "remote.origin.fetch"])
    );
    assert_eq!(
        git(&zmin_mirror, ["config", "--get", "remote.origin.mirror"]),
        git(&git_mirror, ["config", "--get", "remote.origin.mirror"])
    );
    assert_eq!(
        git(&zmin_mirror, ["config", "--get", "remote.origin.tagOpt"]),
        git(&git_mirror, ["config", "--get", "remote.origin.tagOpt"])
    );

    git(
        dir.path(),
        [
            "clone",
            "--shared",
            "--mirror",
            source.to_str().expect("source path"),
            "git-shared-mirror.git",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--shared",
            "--mirror",
            source.to_str().expect("source path"),
            "zmin-shared-mirror.git",
        ],
    );
    let zmin_shared_mirror = dir.path().join("zmin-shared-mirror.git");
    let git_shared_mirror = dir.path().join("git-shared-mirror.git");
    assert_eq!(
        canonical_alternates(&zmin_shared_mirror.join("objects/info/alternates")),
        canonical_alternates(&git_shared_mirror.join("objects/info/alternates"))
    );
    assert_eq!(
        git(&zmin_shared_mirror, ["show-ref"]),
        git(&git_shared_mirror, ["show-ref"])
    );
    assert_eq!(
        git(
            &zmin_shared_mirror,
            ["config", "--get", "remote.origin.mirror"]
        ),
        git(
            &git_shared_mirror,
            ["config", "--get", "remote.origin.mirror"]
        )
    );

    git(
        dir.path(),
        [
            "clone",
            "--mirror",
            "--no-tags",
            source.to_str().expect("source path"),
            "git-mirror-no-tags.git",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--mirror",
            "--no-tags",
            source.to_str().expect("source path"),
            "zmin-mirror-no-tags.git",
        ],
    );
    let zmin_mirror_no_tags = dir.path().join("zmin-mirror-no-tags.git");
    let git_mirror_no_tags = dir.path().join("git-mirror-no-tags.git");
    assert_eq!(
        git(&zmin_mirror_no_tags, ["show-ref"]),
        git(&git_mirror_no_tags, ["show-ref"])
    );
    assert_eq!(
        git(
            &zmin_mirror_no_tags,
            ["config", "--get", "remote.origin.fetch"]
        ),
        git(
            &git_mirror_no_tags,
            ["config", "--get", "remote.origin.fetch"]
        )
    );
}

#[test]
fn clone_long_options_and_checkout_order_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    git(&source, ["switch", "-c", "feature"]);
    fs::write(source.join("feature.txt"), b"feature\n").expect("write feature");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "feature"]);
    git(&source, ["switch", "main"]);

    git(
        dir.path(),
        [
            "clone",
            "--origin",
            "upstream",
            "--branch",
            "feature",
            "--single-branch",
            source.to_str().expect("source path"),
            "git-long-options",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--origin",
            "upstream",
            "--branch",
            "feature",
            "--single-branch",
            source.to_str().expect("source path"),
            "zmin-long-options",
        ],
    );
    let git_long = dir.path().join("git-long-options");
    let zmin_long = dir.path().join("zmin-long-options");
    assert_eq!(
        run_zmin(&zmin_long, ["branch", "--show-current"]),
        git(&git_long, ["branch", "--show-current"])
    );
    assert_eq!(
        run_zmin(&zmin_long, ["branch", "-r"]),
        git(&git_long, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(&zmin_long, ["remote", "-v"]),
        git(&git_long, ["remote", "-v"])
    );
    assert_eq!(
        run_zmin(&zmin_long, ["config", "--get", "branch.feature.remote"]),
        git(&git_long, ["config", "--get", "branch.feature.remote"])
    );

    git(
        dir.path(),
        [
            "clone",
            "--origin=upstream",
            "--branch=feature",
            "--config=core.autocrlf=input",
            "--single-branch",
            source.to_str().expect("source path"),
            "git-equals-options",
        ],
    );
    run_zmin(
        dir.path(),
        [
            "clone",
            "--origin=upstream",
            "--branch=feature",
            "--config=core.autocrlf=input",
            "--single-branch",
            source.to_str().expect("source path"),
            "zmin-equals-options",
        ],
    );
    let git_equals = dir.path().join("git-equals-options");
    let zmin_equals = dir.path().join("zmin-equals-options");
    assert_eq!(
        run_zmin(&zmin_equals, ["branch", "--show-current"]),
        git(&git_equals, ["branch", "--show-current"])
    );
    assert_eq!(
        run_zmin(&zmin_equals, ["branch", "-r"]),
        git(&git_equals, ["branch", "-r"])
    );
    assert_eq!(
        run_zmin(&zmin_equals, ["config", "--get", "branch.feature.remote"]),
        git(&git_equals, ["config", "--get", "branch.feature.remote"])
    );
    assert_eq!(
        run_zmin(&zmin_equals, ["config", "--get", "core.autocrlf"]),
        git(&git_equals, ["config", "--get", "core.autocrlf"])
    );

    for (args, git_name, zmin_name) in [
        (
            ["--no-checkout", "--checkout"].as_slice(),
            "git-checkout-last",
            "zmin-checkout-last",
        ),
        (
            ["--checkout", "--no-checkout"].as_slice(),
            "git-no-checkout-last",
            "zmin-no-checkout-last",
        ),
    ] {
        let mut git_clone_args = vec!["clone"];
        git_clone_args.extend_from_slice(args);
        git_clone_args.extend_from_slice(&[source.to_str().expect("source path"), git_name]);
        let mut zmin_args = vec!["clone"];
        zmin_args.extend_from_slice(args);
        zmin_args.extend_from_slice(&[source.to_str().expect("source path"), zmin_name]);
        git_args(dir.path(), &git_clone_args);
        run_zmin_args(dir.path(), &zmin_args);

        let git_clone = dir.path().join(git_name);
        let zmin_clone = dir.path().join(zmin_name);
        assert_eq!(
            visible_worktree_files(&zmin_clone),
            visible_worktree_files(&git_clone),
            "checkout order worktree mismatch for {args:?}"
        );
        assert_eq!(
            zmin_clone.join(".git/index").exists(),
            git_clone.join(".git/index").exists(),
            "checkout order index mismatch for {args:?}"
        );
        assert_eq!(
            run_zmin(&zmin_clone, ["status", "--porcelain=v1", "--branch"]),
            git(&git_clone, ["status", "--porcelain=v1", "--branch"]),
            "checkout order status mismatch for {args:?}"
        );
    }
}

#[test]
fn clone_documented_local_tail_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("root.txt"), b"root\n").expect("write root");
    fs::create_dir_all(source.join("dir/sub")).expect("create nested dir");
    fs::write(source.join("dir/sub/nested.txt"), b"nested\n").expect("write nested");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);

    let git_root = dir.path().join("git-root");
    let zmin_root = dir.path().join("zmin-root");
    fs::create_dir_all(&git_root).expect("create git root");
    fs::create_dir_all(&zmin_root).expect("create zmin root");

    let assert_success = |name: &str, flags: &[&str], sparse_expected: bool| {
        let mut git_args_vec = vec!["clone"];
        git_args_vec.extend_from_slice(flags);
        git_args_vec.push(source.to_str().expect("source path"));
        git_args_vec.push(name);

        let mut zmin_args_vec = vec!["clone"];
        zmin_args_vec.extend_from_slice(flags);
        zmin_args_vec.push(source.to_str().expect("source path"));
        zmin_args_vec.push(name);

        assert_eq!(
            command_output("git", &git_root, &git_args_vec, "git"),
            command_output(zmin_bin(), &zmin_root, &zmin_args_vec, "zmin")
        );

        let git_clone = git_root.join(name);
        let zmin_clone = zmin_root.join(name);
        assert_eq!(
            run_zmin(&zmin_clone, ["rev-parse", "HEAD"]),
            git(&git_clone, ["rev-parse", "HEAD"]),
            "HEAD mismatch for {flags:?}"
        );
        assert_eq!(
            run_zmin(&zmin_clone, ["status", "--porcelain=v1", "--branch"]),
            git(&git_clone, ["status", "--porcelain=v1", "--branch"]),
            "status mismatch for {flags:?}"
        );
        assert_eq!(
            run_zmin(&zmin_clone, ["config", "--get", "remote.origin.url"]),
            git(&git_clone, ["config", "--get", "remote.origin.url"]),
            "origin URL mismatch for {flags:?}"
        );
        assert_eq!(
            visible_worktree_files(&zmin_clone),
            visible_worktree_files(&git_clone),
            "visible worktree mismatch for {flags:?}"
        );

        if sparse_expected {
            for key in ["core.sparseCheckout", "core.sparseCheckoutCone"] {
                assert_eq!(
                    run_zmin(&zmin_clone, ["config", "--get", key]),
                    git(&git_clone, ["config", "--get", key]),
                    "sparse config mismatch for {key}"
                );
            }
            assert_eq!(
                fs::read_to_string(zmin_clone.join(".git/info/sparse-checkout"))
                    .expect("read zmin sparse-checkout"),
                fs::read_to_string(git_clone.join(".git/info/sparse-checkout"))
                    .expect("read git sparse-checkout"),
                "sparse-checkout file mismatch"
            );
            assert_eq!(
                fs::read_to_string(zmin_clone.join(".git/config.worktree"))
                    .expect("read zmin config.worktree"),
                fs::read_to_string(git_clone.join(".git/config.worktree"))
                    .expect("read git config.worktree"),
                "config.worktree mismatch"
            );
        }
    };

    let assert_failure = |name: &str, flags: &[&str]| {
        let mut git_args_vec = vec!["clone"];
        git_args_vec.extend_from_slice(flags);
        git_args_vec.push(source.to_str().expect("source path"));
        git_args_vec.push(name);

        let mut zmin_args_vec = vec!["clone"];
        zmin_args_vec.extend_from_slice(flags);
        zmin_args_vec.push(source.to_str().expect("source path"));
        zmin_args_vec.push(name);

        assert_eq!(
            command_output("git", &git_root, &git_args_vec, "git"),
            command_output(zmin_bin(), &zmin_root, &zmin_args_vec, "zmin")
        );
        assert!(
            !git_root.join(name).exists(),
            "git failure should not create {name}"
        );
        assert!(
            !zmin_root.join(name).exists(),
            "zmin failure should not create {name}"
        );
    };

    for (name, flags, sparse_expected) in [
        (
            "upload-pack-long",
            ["--upload-pack=git-upload-pack"].as_slice(),
            false,
        ),
        (
            "upload-pack-short",
            ["-u", "git-upload-pack"].as_slice(),
            false,
        ),
        ("server-option", ["--server-option=trace"].as_slice(), false),
        ("filter", ["--filter=blob:none"].as_slice(), false),
        (
            "shallow-since",
            ["--shallow-since=2024-01-01"].as_slice(),
            false,
        ),
        (
            "shallow-exclude",
            ["--shallow-exclude=main"].as_slice(),
            false,
        ),
        (
            "bundle-uri",
            ["--bundle-uri=file:///tmp/missing.bundle"].as_slice(),
            false,
        ),
        (
            "no-remote-submodules",
            ["--no-remote-submodules"].as_slice(),
            false,
        ),
        (
            "no-shallow-submodules",
            ["--no-shallow-submodules"].as_slice(),
            false,
        ),
        (
            "also-filter-submodules",
            [
                "--filter=blob:none",
                "--recurse-submodules",
                "--also-filter-submodules",
            ]
            .as_slice(),
            false,
        ),
        ("sparse", ["--sparse"].as_slice(), true),
    ] {
        assert_success(name, flags, sparse_expected);
    }

    assert_failure(
        "also-filter-missing-recurse",
        ["--filter=blob:none", "--also-filter-submodules"].as_slice(),
    );
    assert_failure(
        "also-filter-missing-filter",
        ["--recurse-submodules", "--also-filter-submodules"].as_slice(),
    );
}

fn canonical_alternates(path: &std::path::Path) -> Vec<std::path::PathBuf> {
    fs::read_to_string(path)
        .expect("read alternates")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(std::path::PathBuf::from)
        .map(|path| fs::canonicalize(&path).unwrap_or(path))
        .collect()
}

fn visible_worktree_files(path: &std::path::Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_visible_worktree_files(path, path, &mut files);
    files.sort();
    files
}

fn collect_visible_worktree_files(
    root: &std::path::Path,
    path: &std::path::Path,
    files: &mut Vec<String>,
) {
    for entry in fs::read_dir(path).expect("read worktree dir") {
        let entry = entry.expect("read worktree entry");
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_visible_worktree_files(root, &path, files);
        } else if path.is_file() {
            files.push(
                path.strip_prefix(root)
                    .expect("strip root")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

fn configure_identity(cwd: &std::path::Path) {
    git(cwd, ["config", "user.name", "Bench"]);
    git(cwd, ["config", "user.email", "bench@example.test"]);
    git(cwd, ["config", "commit.gpgsign", "false"]);
}

#[test]
fn clone_from_partial_local_promisor_matches_stock_git_lazy_fetch_contract() {
    fn setup_partial_promisor_source() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().expect("temp dir");
        let source = dir.path().join("tmp");
        let evil = dir.path().join("evil");
        git(
            dir.path(),
            ["init", "-b", "main", source.to_str().expect("source path")],
        );
        fs::write(source.join("a"), b"a\n").expect("write source file");
        git(&source, ["add", "a"]);
        git_with_env(&source, ["commit", "-m", "a"]);
        git(&source, ["config", "uploadpack.allowfilter", "1"]);
        git(
            dir.path(),
            [
                "clone",
                "--filter=blob:none",
                "--no-local",
                "--no-checkout",
                source.to_str().expect("source path"),
                evil.to_str().expect("evil path"),
            ],
        );
        let fake_upload_pack = dir.path().join("fake-upload-pack");
        fs::write(
            &fake_upload_pack,
            b"#!/bin/sh\necho >&2 \"fake-upload-pack running\"\n>\"$TRASH_DIRECTORY/script-executed\"\nexit 1\n",
        )
        .expect("write fake upload-pack");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&fake_upload_pack)
                .expect("fake upload-pack metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&fake_upload_pack, permissions).expect("chmod fake upload-pack");
        }
        git(
            &evil,
            [
                "config",
                "remote.origin.uploadpack",
                "\"$TRASH_DIRECTORY/fake-upload-pack\"",
            ],
        );
        fs::write(evil.join(".git/shallow"), b"").expect("write shallow marker");
        (dir, evil)
    }

    fn clone_with_env(
        command: &str,
        label: &str,
        envs: &[(&str, &str)],
    ) -> (i32, String, String, bool) {
        let (dir, _evil) = setup_partial_promisor_source();
        let script_executed = dir.path().join("script-executed");
        let mut merged_envs = Vec::with_capacity(envs.len() + 1);
        let trash_directory = dir.path().to_str().expect("trash dir utf8").to_owned();
        merged_envs.push(("TRASH_DIRECTORY", trash_directory.as_str()));
        merged_envs.extend_from_slice(envs);
        let output = command_output_with_env(
            command,
            dir.path(),
            &["clone", "evil", label],
            &merged_envs,
            label,
        );
        let script_present = script_executed.exists();
        (output.0, output.1, output.2, script_present)
    }

    let git_no_lazy = clone_with_env(
        common::stock_git_bin().to_str().expect("stock git utf8"),
        "git-no-lazy",
        &[("GIT_TEST_PACK_PATH_WALK", "0")],
    );
    let zmin_no_lazy = clone_with_env(
        zmin_bin(),
        "zmin-no-lazy",
        &[("GIT_TEST_PACK_PATH_WALK", "0")],
    );
    assert_eq!(zmin_no_lazy.0, git_no_lazy.0, "default no-lazy exit code");
    assert_eq!(zmin_no_lazy.1, git_no_lazy.1, "default no-lazy stdout");
    assert_eq!(
        zmin_no_lazy.3, git_no_lazy.3,
        "default no-lazy script execution parity"
    );
    assert!(
        git_no_lazy.2.contains("lazy fetching disabled"),
        "stock Git stderr missing lazy-fetch warning: {:?}",
        git_no_lazy
    );
    assert!(
        zmin_no_lazy.2.contains("lazy fetching disabled"),
        "Zmin stderr missing lazy-fetch warning: {:?}",
        zmin_no_lazy
    );

    let git_lazy_ok = clone_with_env(
        common::stock_git_bin().to_str().expect("stock git utf8"),
        "git-lazy-ok",
        &[("GIT_NO_LAZY_FETCH", "0")],
    );
    let zmin_lazy_ok = clone_with_env(zmin_bin(), "zmin-lazy-ok", &[("GIT_NO_LAZY_FETCH", "0")]);
    assert_eq!(zmin_lazy_ok.0, git_lazy_ok.0, "lazy-ok exit code");
    assert_eq!(zmin_lazy_ok.1, git_lazy_ok.1, "lazy-ok stdout");
    assert_eq!(
        zmin_lazy_ok.3, git_lazy_ok.3,
        "lazy-ok script execution parity"
    );
    assert!(
        git_lazy_ok.2.contains("fake-upload-pack running"),
        "stock Git stderr missing fake upload-pack marker: {:?}",
        git_lazy_ok
    );
    assert!(
        zmin_lazy_ok.2.contains("fake-upload-pack running"),
        "Zmin stderr missing fake upload-pack marker: {:?}",
        zmin_lazy_ok
    );

    let (dir, _evil) = setup_partial_promisor_source();
    let script_executed = dir.path().join("script-executed");
    let git_pack = command_output_with_env_and_stdin(
        common::stock_git_bin().to_str().expect("stock git utf8"),
        dir.path(),
        &["-C", "evil", "pack-objects", "--revs", "--stdout"],
        &[(
            "TRASH_DIRECTORY",
            dir.path().to_str().expect("trash dir utf8"),
        )],
        "HEAD\n",
        "git pack-objects partial promisor",
    );
    let git_pack_script = script_executed.exists();
    fs::remove_file(&script_executed).expect("remove git script marker");
    let zmin_pack = command_output_with_env_and_stdin(
        zmin_bin(),
        dir.path(),
        &["-C", "evil", "pack-objects", "--revs", "--stdout"],
        &[(
            "TRASH_DIRECTORY",
            dir.path().to_str().expect("trash dir utf8"),
        )],
        "HEAD\n",
        "zmin pack-objects partial promisor",
    );
    let zmin_pack_script = script_executed.exists();
    assert_eq!(zmin_pack.0, git_pack.0, "pack-objects exit code");
    assert_eq!(zmin_pack.1, git_pack.1, "pack-objects stdout");
    assert_eq!(
        zmin_pack_script, git_pack_script,
        "pack-objects script execution parity"
    );
    assert!(
        git_pack.2.contains("fake-upload-pack running"),
        "stock Git stderr missing fake upload-pack marker for pack-objects: {:?}",
        git_pack
    );
    assert!(
        zmin_pack.2.contains("fake-upload-pack running"),
        "Zmin stderr missing fake upload-pack marker for pack-objects: {:?}",
        zmin_pack
    );
}

#[test]
fn clone_filter_empty_file_repo_matches_stock_git() {
    let git_root = TempDir::new().expect("git root");
    let zmin_root = TempDir::new().expect("zmin root");
    let git_source = git_root.path().join("source");
    let zmin_source = zmin_root.path().join("source");

    git(
        git_root.path(),
        [
            "init",
            "--bare",
            git_source.to_str().expect("git source path"),
        ],
    );
    git(
        zmin_root.path(),
        [
            "init",
            "--bare",
            zmin_source.to_str().expect("zmin source path"),
        ],
    );

    let git_output = command_output(
        "git",
        git_root.path(),
        &[
            "clone",
            "--filter=blob:none",
            &format!("file://{}", git_source.display()),
            "client",
        ],
        "git clone filter empty file repo",
    );
    let zmin_output = command_output(
        zmin_bin(),
        zmin_root.path(),
        &[
            "clone",
            "--filter=blob:none",
            &format!("file://{}", zmin_source.display()),
            "client",
        ],
        "zmin clone filter empty file repo",
    );

    assert_eq!(zmin_output, git_output);
    assert_eq!(
        git_root.path().join("client").exists(),
        zmin_root.path().join("client").exists()
    );
    assert_eq!(
        git(
            &git_root.path().join("client"),
            ["rev-parse", "--is-shallow-repository"]
        ),
        git(
            &zmin_root.path().join("client"),
            ["rev-parse", "--is-shallow-repository"]
        )
    );

    let git_client = git_root.path().join("client");
    let zmin_client = zmin_root.path().join("client");
    git(
        &git_client,
        ["config", "--unset", "remote.origin.partialclonefilter"],
    );
    run_zmin(
        &zmin_client,
        ["config", "--unset", "remote.origin.partialclonefilter"],
    );

    let git_fetch = command_output("git", &git_client, &["fetch", "origin"], "git fetch origin");
    let zmin_fetch = command_output(
        zmin_bin(),
        &zmin_client,
        &["fetch", "origin"],
        "zmin fetch origin",
    );
    assert_eq!(zmin_fetch, git_fetch);
    assert_eq!(
        fs::read_to_string(git_client.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD"),
        fs::read_to_string(zmin_client.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD")
    );
}

#[test]
fn clone_filter_nonempty_file_repo_matches_stock_git() {
    let git_root = TempDir::new().expect("git root");
    let zmin_root = TempDir::new().expect("zmin root");
    let git_source = git_root.path().join("source");
    let zmin_source = zmin_root.path().join("source");

    git(
        git_root.path(),
        [
            "init",
            "-b",
            "main",
            git_source.to_str().expect("git source path"),
        ],
    );
    configure_identity(&git_source);
    fs::write(git_source.join("server1.t"), b"one\n").expect("write git source file");
    git(&git_source, ["add", "-A"]);
    git_with_env(&git_source, ["commit", "-m", "server1"]);
    git(&git_source, ["config", "uploadpack.allowfilter", "1"]);
    git(
        &git_source,
        ["config", "uploadpack.allowanysha1inwant", "1"],
    );

    run_zmin(
        zmin_root.path(),
        [
            "init",
            "-b",
            "main",
            zmin_source.to_str().expect("zmin source path"),
        ],
    );
    configure_identity(&zmin_source);
    fs::write(zmin_source.join("server1.t"), b"one\n").expect("write zmin source file");
    run_zmin(&zmin_source, ["add", "-A"]);
    run_zmin(&zmin_source, ["commit", "-m", "server1"]);
    run_zmin(&zmin_source, ["config", "uploadpack.allowfilter", "1"]);
    run_zmin(
        &zmin_source,
        ["config", "uploadpack.allowanysha1inwant", "1"],
    );

    let git_output = command_output(
        "git",
        git_root.path(),
        &[
            "clone",
            "--filter=blob:none",
            &format!("file://{}", git_source.display()),
            "client",
        ],
        "git clone filter nonempty file repo",
    );
    let zmin_output = command_output(
        zmin_bin(),
        zmin_root.path(),
        &[
            "clone",
            "--filter=blob:none",
            &format!("file://{}", zmin_source.display()),
            "client",
        ],
        "zmin clone filter nonempty file repo",
    );

    assert_eq!(zmin_output, git_output);

    let git_client = git_root.path().join("client");
    let zmin_client = zmin_root.path().join("client");
    assert_eq!(
        git(&zmin_client, ["status", "--porcelain=v1", "--branch"]),
        git(&git_client, ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read_to_string(zmin_client.join("server1.t")).expect("zmin checkout file"),
        fs::read_to_string(git_client.join("server1.t")).expect("git checkout file")
    );
    assert_eq!(
        git(&zmin_client, ["cat-file", "-t", "HEAD^{tree}"]),
        git(&git_client, ["cat-file", "-t", "HEAD^{tree}"])
    );
}

fn create_clone_source(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    let source = root.join(name);
    git(
        root,
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("README.md"), b"main\n").expect("write main");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "main"]);
    source
}

fn first_loose_object(objects_dir: &std::path::Path) -> std::path::PathBuf {
    let mut stack = vec![objects_dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(path).expect("read object dir") {
            let entry = entry.expect("object dir entry");
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("object metadata");
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file()
                && !path
                    .strip_prefix(objects_dir)
                    .expect("object prefix")
                    .starts_with("info")
                && !path
                    .strip_prefix(objects_dir)
                    .expect("object prefix")
                    .starts_with("pack")
            {
                return path;
            }
        }
    }
    panic!("loose object not found");
}

fn run_zmin<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    run_zmin_args(cwd, &args)
}

fn run_zmin_args(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new(zmin_bin())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run zmin");
    assert!(
        output.status.success(),
        "zmin failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("zmin stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn run_zmin_status<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> i32 {
    run_zmin_status_args(cwd, &args)
}

fn run_zmin_status_args(cwd: &std::path::Path, args: &[&str]) -> i32 {
    Command::new(zmin_bin())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run zmin")
        .status
        .code()
        .expect("zmin exited by signal")
}

fn command_output(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    label: &str,
) -> (i32, String, String) {
    command_output_with_env(command, cwd, args, &[], label)
}

fn command_output_with_env(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
    label: &str,
) -> (i32, String, String) {
    let output = Command::new(common::test_command_program(command))
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

fn git_with_stdin(cwd: &std::path::Path, args: &[&str], input: &str) -> String {
    use std::io::Write;

    let mut child = Command::new(common::stock_git_bin())
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run git");
    child
        .stdin
        .as_mut()
        .expect("git stdin")
        .write_all(input.as_bytes())
        .expect("write git stdin");
    let output = child.wait_with_output().expect("wait git");
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

fn command_output_with_env_and_stdin(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
    input: &str,
    label: &str,
) -> (i32, String, String) {
    use std::io::Write;

    let mut child = Command::new(common::test_command_program(command))
        .args(args)
        .envs(envs.iter().copied())
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("run {label}: {err}"));
    child
        .stdin
        .as_mut()
        .expect("command stdin")
        .write_all(input.as_bytes())
        .unwrap_or_else(|err| panic!("write stdin for {label}: {err}"));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("wait {label}: {err}"));
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

#[cfg(unix)]
fn case_insensitive_filesystem(root: &std::path::Path) -> bool {
    let probe = root.join("case-insensitive-probe");
    fs::create_dir(&probe).expect("create case probe");
    fs::write(probe.join("CamelCase"), b"good\n").expect("write uppercase probe");
    fs::write(probe.join("camelcase"), b"bad\n").expect("write lowercase probe");
    fs::read(probe.join("CamelCase")).expect("read uppercase probe") != b"good\n"
}

fn git_with_env<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    let output = Command::new(common::stock_git_bin())
        .args(args)
        .env("GIT_AUTHOR_NAME", "Bench")
        .env("GIT_AUTHOR_EMAIL", "bench@example.test")
        .env("GIT_COMMITTER_NAME", "Bench")
        .env("GIT_COMMITTER_EMAIL", "bench@example.test")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .current_dir(cwd)
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

fn git<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    git_args(cwd, &args)
}

fn git_args(cwd: &std::path::Path, args: &[&str]) -> String {
    git_args_with_env(cwd, args, &[])
}

fn git_args_with_env(cwd: &std::path::Path, args: &[&str], envs: &[(&str, &str)]) -> String {
    let output = Command::new(common::stock_git_bin())
        .args(args)
        .envs(envs.iter().copied())
        .current_dir(cwd)
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
