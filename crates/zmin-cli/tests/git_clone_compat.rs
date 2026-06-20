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
fn clone_ref_format_files_is_accepted_and_reftable_is_explicitly_unsupported() {
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

    let reftable = command_output(
        zmin_bin(),
        dir.path(),
        &[
            "clone",
            "--ref-format=reftable",
            source.to_str().expect("source path"),
            "zmin-reftable",
        ],
        "zmin",
    );
    assert_eq!(reftable.0, 128);
    assert!(
        reftable
            .2
            .contains("reftable ref storage is not supported yet")
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
        &[
            "clone",
            source.to_str().expect("source path"),
            "zmin-entry",
        ],
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
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-clone",
        ],
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
        &[
            "clone",
            source.to_str().expect("source path"),
            "zmin-clone",
        ],
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
        [
            "clone",
            source.to_str().expect("source path"),
            "zmin-clone",
        ],
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
        (
            "--hardlinks",
            "git-hardlinks-clone",
            "zmin-hardlinks-clone",
        ),
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
        run_zmin(
            &zmin_config_clone,
            ["status", "--porcelain=v1", "--branch"]
        ),
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
        command_output(
            zmin_bin(),
            &zmin_no_tags,
            &["show-ref", "--tags"],
            "zmin"
        ),
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
        run_zmin(
            &zmin_shared_clone,
            ["status", "--porcelain=v1", "--branch"]
        ),
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
        [
            "clone",
            "--depth",
            "1",
            &source_file_url,
            "zmin-file-clone",
        ],
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
        run_zmin(
            &zmin_single_tag,
            ["config", "--get", "remote.origin.fetch"]
        ),
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
        fs::read_to_string(zmin_file_no_single_branch.join(".git/shallow"))
            .expect("zmin shallow"),
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
    let output = Command::new(common::stock_git_bin())
        .args(args)
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
