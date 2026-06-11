use std::fs;
use std::process::Command;

use tempfile::TempDir;

fn skron_bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_skron-git").unwrap_or(env!("CARGO_BIN_EXE_skron"))
}

#[test]
fn clone_unsupported_remote_helper_failure_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");

    assert_eq!(
        command_output(
            skron_bin(),
            dir.path(),
            &["clone", "skronproto://example/repo", "dst"],
            "skron",
        ),
        command_output(
            "git",
            dir.path(),
            &["clone", "skronproto://example/repo", "dst"],
            "git",
        )
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
    let skron_root = command_output(
        skron_bin(),
        dir.path(),
        &["clone", source.to_str().expect("source path"), "skron-root"],
        "skron",
    );
    assert_eq!(skron_root.0, git_root.0);
    assert!(skron_root.2.contains("refusing to clone with --local"));

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
    let skron_entry = command_output(
        skron_bin(),
        dir.path(),
        &[
            "clone",
            source.to_str().expect("source path"),
            "skron-entry",
        ],
        "skron",
    );
    assert_eq!(skron_entry.0, git_entry.0);
    assert!(skron_entry.2.contains("refusing to clone with --local"));
}

#[cfg(unix)]
#[test]
fn fetch_rejects_symlinked_destination_object_store() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().expect("temp dir");
    let source = create_clone_source(dir.path(), "source");
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-clone",
        ],
    );
    let clone = dir.path().join("skron-clone");
    let loose_object = first_loose_object(&source.join(".git/objects"));
    let object_dir = loose_object
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .expect("loose object dir");
    let destination_object_dir = clone.join(".git/objects").join(object_dir);
    fs::remove_dir_all(&destination_object_dir).expect("remove destination object dir");
    symlink(dir.path(), &destination_object_dir).expect("symlink destination object dir");

    let output = command_output(skron_bin(), &clone, &["fetch", "origin"], "skron");

    assert_eq!(output.0, 128);
    assert!(output.2.contains("destination object path"));
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
    run_skron(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            "skron-clone",
        ],
    );
    let git_clone = dir.path().join("git-clone");
    let skron_clone = dir.path().join("skron-clone");

    assert_eq!(
        git(skron_clone.as_path(), ["rev-parse", "HEAD"]),
        git(git_clone.as_path(), ["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(skron_clone.as_path(), ["cat-file", "-p", "HEAD^{tree}"]),
        git(git_clone.as_path(), ["cat-file", "-p", "HEAD^{tree}"])
    );
    assert_eq!(
        run_skron(skron_clone.as_path(), ["remote", "-v"]),
        git(git_clone.as_path(), ["remote", "-v"])
    );
    assert_eq!(
        run_skron(skron_clone.as_path(), ["branch", "-r"]),
        git(git_clone.as_path(), ["branch", "-r"])
    );
    assert_eq!(
        run_skron(
            skron_clone.as_path(),
            ["status", "--porcelain=v1", "--branch"]
        ),
        git(
            git_clone.as_path(),
            ["status", "--porcelain=v1", "--branch"]
        )
    );

    assert_eq!(
        command_output(
            skron_bin(),
            dir.path(),
            &[
                "clone",
                "--quiet",
                source.to_str().expect("source path"),
                "skron-quiet-clone",
            ],
            "skron"
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

    for (flag, git_dir_name, skron_dir_name) in [
        ("--local", "git-local-clone", "skron-local-clone"),
        ("--no-local", "git-no-local-clone", "skron-no-local-clone"),
        (
            "--no-hardlinks",
            "git-no-hardlinks-clone",
            "skron-no-hardlinks-clone",
        ),
        (
            "--hardlinks",
            "git-hardlinks-clone",
            "skron-hardlinks-clone",
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
        run_skron(
            dir.path(),
            [
                "clone",
                flag,
                source.to_str().expect("source path"),
                skron_dir_name,
            ],
        );
        assert_eq!(
            run_skron(
                &dir.path().join(skron_dir_name),
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
    run_skron(
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
            "skron-config-clone",
        ],
    );
    let git_config_clone = dir.path().join("git-config-clone");
    let skron_config_clone = dir.path().join("skron-config-clone");
    for key in [
        "core.autocrlf",
        "clone.flag.without.value",
        "remote.origin.tagOpt",
    ] {
        assert_eq!(
            run_skron(&skron_config_clone, ["config", "--get", key]),
            git(&git_config_clone, ["config", "--get", key]),
            "clone config mismatch for {key}"
        );
    }
    assert_eq!(
        run_skron(
            &skron_config_clone,
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--template",
            template.to_str().expect("template path"),
            source.to_str().expect("source path"),
            "skron-template-clone",
        ],
    );
    let git_template_clone = dir.path().join("git-template-clone");
    let skron_template_clone = dir.path().join("skron-template-clone");
    for path in [
        ".git/hooks/pre-commit",
        ".git/info/exclude",
        ".git/description",
    ] {
        assert_eq!(
            fs::read_to_string(skron_template_clone.join(path)).expect("skron template file"),
            fs::read_to_string(git_template_clone.join(path)).expect("git template file"),
            "template file mismatch for {path}"
        );
    }
    assert_eq!(
        run_skron(&skron_template_clone, ["config", "--get", "custom.value"]),
        git(&git_template_clone, ["config", "--get", "custom.value"])
    );
    assert_eq!(
        run_skron(
            &skron_template_clone,
            ["status", "--porcelain=v1", "--branch"]
        ),
        git(
            &git_template_clone,
            ["status", "--porcelain=v1", "--branch"]
        )
    );
    for (args, git_name, skron_name) in [
        (
            [
                "--no-template",
                "--template",
                template.to_str().expect("template path"),
            ]
            .as_slice(),
            "git-template-last",
            "skron-template-last",
        ),
        (
            [
                "--template",
                template.to_str().expect("template path"),
                "--no-template",
            ]
            .as_slice(),
            "git-no-template-last",
            "skron-no-template-last",
        ),
    ] {
        let mut git_clone_args = vec!["clone"];
        git_clone_args.extend_from_slice(args);
        git_clone_args.extend_from_slice(&[source.to_str().expect("source path"), git_name]);
        let mut skron_clone_args = vec!["clone"];
        skron_clone_args.extend_from_slice(args);
        skron_clone_args.extend_from_slice(&[source.to_str().expect("source path"), skron_name]);
        git_args(dir.path(), &git_clone_args);
        run_skron_args(dir.path(), &skron_clone_args);
        let git_clone = dir.path().join(git_name);
        let skron_clone = dir.path().join(skron_name);
        assert_eq!(
            command_output(
                skron_bin(),
                &skron_clone,
                &["config", "--get", "custom.value"],
                "skron"
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
            skron_clone.join(".git/hooks/pre-commit").exists(),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "-o",
            "upstream",
            source.to_str().expect("source path"),
            "skron-origin-clone",
        ],
    );
    let skron_origin_clone = dir.path().join("skron-origin-clone");
    let git_origin_clone = dir.path().join("git-origin-clone");
    assert_eq!(
        run_skron(&skron_origin_clone, ["remote", "-v"]),
        git(&git_origin_clone, ["remote", "-v"])
    );
    assert_eq!(
        run_skron(&skron_origin_clone, ["branch", "-r"]),
        git(&git_origin_clone, ["branch", "-r"])
    );
    assert_eq!(
        run_skron(
            &skron_origin_clone,
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--no-tags",
            source.to_str().expect("source path"),
            "skron-no-tags",
        ],
    );
    let skron_no_tags = dir.path().join("skron-no-tags");
    let git_no_tags = dir.path().join("git-no-tags");
    assert_eq!(
        command_output(
            skron_bin(),
            &skron_no_tags,
            &["show-ref", "--tags"],
            "skron"
        ),
        command_output("git", &git_no_tags, &["show-ref", "--tags"], "git")
    );
    assert_eq!(
        run_skron(&skron_no_tags, ["config", "--get", "remote.origin.tagOpt"]),
        git(&git_no_tags, ["config", "--get", "remote.origin.tagOpt"])
    );
    for (args, git_name, skron_name) in [
        (["--tags"].as_slice(), "git-tags", "skron-tags"),
        (
            ["--no-tags", "--tags"].as_slice(),
            "git-tags-last",
            "skron-tags-last",
        ),
        (
            ["--tags", "--no-tags"].as_slice(),
            "git-no-tags-last",
            "skron-no-tags-last",
        ),
    ] {
        let mut git_clone_args = vec!["clone"];
        git_clone_args.extend_from_slice(args);
        git_clone_args.extend_from_slice(&[source.to_str().expect("source path"), git_name]);
        let mut skron_clone_args = vec!["clone"];
        skron_clone_args.extend_from_slice(args);
        skron_clone_args.extend_from_slice(&[source.to_str().expect("source path"), skron_name]);
        git_args(dir.path(), &git_clone_args);
        run_skron_args(dir.path(), &skron_clone_args);
        let git_clone = dir.path().join(git_name);
        let skron_clone = dir.path().join(skron_name);
        assert_eq!(
            run_skron(&skron_clone, ["tag"]),
            git(&git_clone, ["tag"]),
            "tag list mismatch for {args:?}"
        );
        assert_eq!(
            command_output(
                skron_bin(),
                &skron_clone,
                &["config", "--get", "remote.origin.tagOpt"],
                "skron"
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--reference",
            reference.to_str().expect("reference path"),
            source.to_str().expect("source path"),
            "skron-reference-clone",
        ],
    );
    let skron_reference_clone = dir.path().join("skron-reference-clone");
    let git_reference_clone = dir.path().join("git-reference-clone");
    assert_eq!(
        fs::read_to_string(skron_reference_clone.join(".git/objects/info/alternates"))
            .expect("skron alternates"),
        fs::read_to_string(git_reference_clone.join(".git/objects/info/alternates"))
            .expect("git alternates")
    );
    assert_eq!(
        run_skron(
            &skron_reference_clone,
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--reference-if-able",
            reference.to_str().expect("reference path"),
            source.to_str().expect("source path"),
            "skron-reference-if-able-clone",
        ],
    );
    let skron_reference_if_able_clone = dir.path().join("skron-reference-if-able-clone");
    let git_reference_if_able_clone = dir.path().join("git-reference-if-able-clone");
    assert_eq!(
        canonical_alternates(&skron_reference_if_able_clone.join(".git/objects/info/alternates")),
        canonical_alternates(&git_reference_if_able_clone.join(".git/objects/info/alternates"))
    );
    assert_eq!(
        run_skron(
            &skron_reference_if_able_clone,
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--reference-if-able",
            "missing-reference",
            source.to_str().expect("source path"),
            "skron-missing-reference-if-able-clone",
        ],
    );
    assert_eq!(
        dir.path()
            .join("skron-missing-reference-if-able-clone/.git/objects/info/alternates")
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--shared",
            source.to_str().expect("source path"),
            "skron-shared-clone",
        ],
    );
    let skron_shared_clone = dir.path().join("skron-shared-clone");
    let git_shared_clone = dir.path().join("git-shared-clone");
    assert_eq!(
        canonical_alternates(&skron_shared_clone.join(".git/objects/info/alternates")),
        canonical_alternates(&git_shared_clone.join(".git/objects/info/alternates"))
    );
    assert_eq!(
        run_skron(
            &skron_shared_clone,
            ["status", "--porcelain=v1", "--branch"]
        ),
        git(&git_shared_clone, ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        run_skron(&skron_shared_clone, ["cat-file", "-t", "HEAD"]),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--reference",
            reference.to_str().expect("reference path"),
            "--dissociate",
            source.to_str().expect("source path"),
            "skron-dissociate-clone",
        ],
    );
    assert_eq!(
        dir.path()
            .join("skron-dissociate-clone/.git/objects/info/alternates")
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--shared",
            "--dissociate",
            source.to_str().expect("source path"),
            "skron-shared-dissociate-clone",
        ],
    );
    assert_eq!(
        dir.path()
            .join("skron-shared-dissociate-clone/.git/objects/info/alternates")
            .exists(),
        dir.path()
            .join("git-shared-dissociate-clone/.git/objects/info/alternates")
            .exists()
    );
    let skron_shared_dissociate = dir.path().join("skron-shared-dissociate-clone");
    let git_shared_dissociate = dir.path().join("git-shared-dissociate-clone");
    assert_eq!(
        run_skron(&skron_shared_dissociate, ["cat-file", "-t", "HEAD"]),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--single-branch",
            source.to_str().expect("source path"),
            "skron-single-branch",
        ],
    );
    let skron_single_branch = dir.path().join("skron-single-branch");
    let git_single_branch = dir.path().join("git-single-branch");
    assert_eq!(
        run_skron(&skron_single_branch, ["branch", "-r"]),
        git(&git_single_branch, ["branch", "-r"])
    );
    assert_eq!(
        run_skron(
            &skron_single_branch,
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
        run_skron_status(
            dir.path(),
            [
                "clone",
                "--depth",
                "1",
                source.to_str().expect("source path"),
                "skron-depth-clone",
            ],
        ),
        0
    );
    let skron_depth_clone = dir.path().join("skron-depth-clone");
    let git_depth_clone = dir.path().join("git-depth-clone");
    assert_eq!(
        git(&skron_depth_clone, ["rev-parse", "HEAD"]),
        git(&git_depth_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        run_skron(&skron_depth_clone, ["branch", "-r"]),
        git(&git_depth_clone, ["branch", "-r"])
    );
    assert_eq!(
        run_skron(
            &skron_depth_clone,
            ["config", "--get", "remote.origin.fetch"]
        ),
        git(&git_depth_clone, ["config", "--get", "remote.origin.fetch"])
    );

    let source_file_url = format!("file://{}", source.display());
    git(
        dir.path(),
        ["clone", "--depth", "1", &source_file_url, "git-file-clone"],
    );
    run_skron(
        dir.path(),
        [
            "clone",
            "--depth",
            "1",
            &source_file_url,
            "skron-file-clone",
        ],
    );
    let skron_file_clone = dir.path().join("skron-file-clone");
    let git_file_clone = dir.path().join("git-file-clone");
    assert_eq!(
        git(&skron_file_clone, ["rev-parse", "HEAD"]),
        git(&git_file_clone, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        run_skron(&skron_file_clone, ["branch", "-r"]),
        git(&git_file_clone, ["branch", "-r"])
    );
    assert_eq!(
        run_skron(&skron_file_clone, ["log", "--oneline", "--all"]),
        git(&git_file_clone, ["log", "--oneline", "--all"])
    );
    assert_eq!(
        fs::read_to_string(skron_file_clone.join(".git/shallow")).expect("skron shallow"),
        fs::read_to_string(git_file_clone.join(".git/shallow")).expect("git shallow")
    );

    assert_eq!(
        command_output(
            skron_bin(),
            dir.path(),
            &[
                "clone",
                "--reject-shallow",
                git_file_clone.to_str().expect("shallow source path"),
                "reject-shallow-target",
            ],
            "skron"
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--no-reject-shallow",
            git_file_clone.to_str().expect("shallow source path"),
            "skron-no-reject-shallow",
        ],
    );
    assert_eq!(
        run_skron(
            &dir.path().join("skron-no-reject-shallow"),
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
        let skron_cwd = dir.path().join(format!("skron-reject-order-{index}"));
        fs::create_dir(&git_cwd).expect("create git reject order dir");
        fs::create_dir(&skron_cwd).expect("create skron reject order dir");
        let mut git_args = vec!["clone"];
        git_args.extend_from_slice(args);
        git_args.extend_from_slice(&[
            git_file_clone.to_str().expect("shallow source path"),
            "target",
        ]);
        let mut skron_args = vec!["clone"];
        skron_args.extend_from_slice(args);
        skron_args.extend_from_slice(&[
            git_file_clone.to_str().expect("shallow source path"),
            "target",
        ]);
        assert_eq!(
            command_output(skron_bin(), &skron_cwd, &skron_args, "skron"),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "skron-branch-clone",
        ],
    );
    let skron_branch_clone = dir.path().join("skron-branch-clone");
    let git_branch_clone = dir.path().join("git-branch-clone");
    assert_eq!(
        run_skron(&skron_branch_clone, ["branch", "--show-current"]),
        git(&git_branch_clone, ["branch", "--show-current"])
    );
    assert_eq!(
        git(&skron_branch_clone, ["cat-file", "-p", "HEAD:feature.txt"]),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--single-branch",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "skron-single-feature",
        ],
    );
    let skron_single_feature = dir.path().join("skron-single-feature");
    let git_single_feature = dir.path().join("git-single-feature");
    assert_eq!(
        run_skron(&skron_single_feature, ["branch", "-r"]),
        git(&git_single_feature, ["branch", "-r"])
    );
    assert_eq!(
        run_skron(
            &skron_single_feature,
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--single-branch",
            "-b",
            "v1",
            source.to_str().expect("source path"),
            "skron-single-tag",
        ],
    );
    let skron_single_tag = dir.path().join("skron-single-tag");
    let git_single_tag = dir.path().join("git-single-tag");
    assert_eq!(
        run_skron(&skron_single_tag, ["rev-parse", "--abbrev-ref", "HEAD"]),
        git(&git_single_tag, ["rev-parse", "--abbrev-ref", "HEAD"])
    );
    assert_eq!(
        run_skron(&skron_single_tag, ["branch", "-r"]),
        git(&git_single_tag, ["branch", "-r"])
    );
    assert_eq!(
        run_skron(
            &skron_single_tag,
            ["config", "--get", "remote.origin.fetch"]
        ),
        git(&git_single_tag, ["config", "--get", "remote.origin.fetch"])
    );
    assert_eq!(
        run_skron(&skron_single_tag, ["show-ref", "--tags"]),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--no-checkout",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "skron-no-checkout",
        ],
    );
    let skron_no_checkout = dir.path().join("skron-no-checkout");
    let git_no_checkout = dir.path().join("git-no-checkout");
    assert_eq!(
        run_skron(&skron_no_checkout, ["branch", "--show-current"]),
        git(&git_no_checkout, ["branch", "--show-current"])
    );
    assert_eq!(
        run_skron(&skron_no_checkout, ["status", "--porcelain=v1", "--branch"]),
        git(&git_no_checkout, ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read_dir(&skron_no_checkout)
            .expect("read skron no checkout")
            .filter(|entry| entry.as_ref().expect("skron no checkout entry").file_name() != ".git")
            .count(),
        fs::read_dir(&git_no_checkout)
            .expect("read git no checkout")
            .filter(|entry| entry.as_ref().expect("git no checkout entry").file_name() != ".git")
            .count()
    );
    assert_eq!(
        skron_no_checkout.join(".git/index").exists(),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--separate-git-dir",
            dir.path()
                .join("skron-separate-meta.git")
                .to_str()
                .expect("skron separate dir"),
            source.to_str().expect("source path"),
            "skron-separate-work",
        ],
    );
    let skron_separate_work = dir.path().join("skron-separate-work");
    let git_separate_work = dir.path().join("git-separate-work");
    let skron_separate_meta = dir.path().join("skron-separate-meta.git");
    let git_separate_meta = dir.path().join("git-separate-meta.git");
    assert!(skron_separate_work.join(".git").is_file());
    assert!(git_separate_work.join(".git").is_file());
    assert_eq!(
        git(&skron_separate_work, ["rev-parse", "HEAD"]),
        git(&git_separate_work, ["rev-parse", "HEAD"])
    );
    assert_eq!(
        run_skron(
            &skron_separate_work,
            ["status", "--porcelain=v1", "--branch"]
        ),
        git(&git_separate_work, ["status", "--porcelain=v1", "--branch"])
    );
    assert_eq!(
        fs::read_to_string(skron_separate_meta.join("HEAD")).expect("skron separate HEAD"),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--depth",
            "1",
            "-b",
            "feature",
            &source_file_url,
            "skron-file-branch-clone",
        ],
    );
    let skron_file_branch_clone = dir.path().join("skron-file-branch-clone");
    let git_file_branch_clone = dir.path().join("git-file-branch-clone");
    assert_eq!(
        run_skron(&skron_file_branch_clone, ["branch", "-a"]),
        git(&git_file_branch_clone, ["branch", "-a"])
    );
    assert_eq!(
        run_skron(&skron_file_branch_clone, ["log", "--oneline", "--all"]),
        git(&git_file_branch_clone, ["log", "--oneline", "--all"])
    );
    assert_eq!(
        fs::read_to_string(skron_file_branch_clone.join(".git/shallow")).expect("skron shallow"),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--depth",
            "1",
            "--no-single-branch",
            &source_file_url,
            "skron-file-no-single-branch",
        ],
    );
    let skron_file_no_single_branch = dir.path().join("skron-file-no-single-branch");
    let git_file_no_single_branch = dir.path().join("git-file-no-single-branch");
    assert_eq!(
        run_skron(&skron_file_no_single_branch, ["branch", "-r"]),
        git(&git_file_no_single_branch, ["branch", "-r"])
    );
    assert_eq!(
        run_skron(
            &skron_file_no_single_branch,
            ["config", "--get", "remote.origin.fetch"]
        ),
        git(
            &git_file_no_single_branch,
            ["config", "--get", "remote.origin.fetch"]
        )
    );
    assert_eq!(
        run_skron(&skron_file_no_single_branch, ["log", "--oneline", "--all"]),
        git(&git_file_no_single_branch, ["log", "--oneline", "--all"])
    );
    assert_eq!(
        fs::read_to_string(skron_file_no_single_branch.join(".git/shallow"))
            .expect("skron shallow"),
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
        let skron_name = format!("skron-single-order-{index}");
        let mut git_clone_args = vec!["clone"];
        git_clone_args.extend_from_slice(args);
        git_clone_args.extend_from_slice(&[&source_file_url, &git_name]);
        let mut skron_clone_args = vec!["clone"];
        skron_clone_args.extend_from_slice(args);
        skron_clone_args.extend_from_slice(&[&source_file_url, &skron_name]);
        git_args(dir.path(), &git_clone_args);
        run_skron_args(dir.path(), &skron_clone_args);

        let git_clone = dir.path().join(git_name);
        let skron_clone = dir.path().join(skron_name);
        assert_eq!(
            run_skron(&skron_clone, ["branch", "-r"]),
            git(&git_clone, ["branch", "-r"]),
            "single-branch order refs mismatch for {args:?}"
        );
        assert_eq!(
            run_skron(&skron_clone, ["config", "--get", "remote.origin.fetch"]),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--bare",
            "-b",
            "feature",
            source.to_str().expect("source path"),
            "skron-bare.git",
        ],
    );
    let skron_bare = dir.path().join("skron-bare.git");
    let git_bare = dir.path().join("git-bare.git");
    assert_eq!(
        git(&skron_bare, ["rev-parse", "--is-bare-repository"]),
        git(&git_bare, ["rev-parse", "--is-bare-repository"])
    );
    assert_eq!(
        git(&skron_bare, ["symbolic-ref", "HEAD"]),
        git(&git_bare, ["symbolic-ref", "HEAD"])
    );
    assert_eq!(git(&skron_bare, ["show-ref"]), git(&git_bare, ["show-ref"]));

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
    run_skron(
        dir.path(),
        [
            "clone",
            "--shared",
            "--bare",
            source.to_str().expect("source path"),
            "skron-shared-bare.git",
        ],
    );
    let skron_shared_bare = dir.path().join("skron-shared-bare.git");
    let git_shared_bare = dir.path().join("git-shared-bare.git");
    assert_eq!(
        canonical_alternates(&skron_shared_bare.join("objects/info/alternates")),
        canonical_alternates(&git_shared_bare.join("objects/info/alternates"))
    );
    assert_eq!(
        git(&skron_shared_bare, ["show-ref"]),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--bare",
            "--no-tags",
            source.to_str().expect("source path"),
            "skron-bare-no-tags.git",
        ],
    );
    let skron_bare_no_tags = dir.path().join("skron-bare-no-tags.git");
    let git_bare_no_tags = dir.path().join("git-bare-no-tags.git");
    assert_eq!(
        command_output("git", &skron_bare_no_tags, &["show-ref", "--tags"], "git"),
        command_output("git", &git_bare_no_tags, &["show-ref", "--tags"], "git")
    );
    assert_eq!(
        git(
            &skron_bare_no_tags,
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--mirror",
            source.to_str().expect("source path"),
            "skron-mirror.git",
        ],
    );
    let skron_mirror = dir.path().join("skron-mirror.git");
    let git_mirror = dir.path().join("git-mirror.git");
    assert_eq!(
        git(&skron_mirror, ["rev-parse", "--is-bare-repository"]),
        git(&git_mirror, ["rev-parse", "--is-bare-repository"])
    );
    assert_eq!(
        git(&skron_mirror, ["symbolic-ref", "HEAD"]),
        git(&git_mirror, ["symbolic-ref", "HEAD"])
    );
    assert_eq!(
        git(&skron_mirror, ["show-ref"]),
        git(&git_mirror, ["show-ref"])
    );
    assert_eq!(
        git(&skron_mirror, ["config", "--get", "remote.origin.fetch"]),
        git(&git_mirror, ["config", "--get", "remote.origin.fetch"])
    );
    assert_eq!(
        git(&skron_mirror, ["config", "--get", "remote.origin.mirror"]),
        git(&git_mirror, ["config", "--get", "remote.origin.mirror"])
    );
    assert_eq!(
        git(&skron_mirror, ["config", "--get", "remote.origin.tagOpt"]),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--shared",
            "--mirror",
            source.to_str().expect("source path"),
            "skron-shared-mirror.git",
        ],
    );
    let skron_shared_mirror = dir.path().join("skron-shared-mirror.git");
    let git_shared_mirror = dir.path().join("git-shared-mirror.git");
    assert_eq!(
        canonical_alternates(&skron_shared_mirror.join("objects/info/alternates")),
        canonical_alternates(&git_shared_mirror.join("objects/info/alternates"))
    );
    assert_eq!(
        git(&skron_shared_mirror, ["show-ref"]),
        git(&git_shared_mirror, ["show-ref"])
    );
    assert_eq!(
        git(
            &skron_shared_mirror,
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--mirror",
            "--no-tags",
            source.to_str().expect("source path"),
            "skron-mirror-no-tags.git",
        ],
    );
    let skron_mirror_no_tags = dir.path().join("skron-mirror-no-tags.git");
    let git_mirror_no_tags = dir.path().join("git-mirror-no-tags.git");
    assert_eq!(
        git(&skron_mirror_no_tags, ["show-ref"]),
        git(&git_mirror_no_tags, ["show-ref"])
    );
    assert_eq!(
        git(
            &skron_mirror_no_tags,
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--origin",
            "upstream",
            "--branch",
            "feature",
            "--single-branch",
            source.to_str().expect("source path"),
            "skron-long-options",
        ],
    );
    let git_long = dir.path().join("git-long-options");
    let skron_long = dir.path().join("skron-long-options");
    assert_eq!(
        run_skron(&skron_long, ["branch", "--show-current"]),
        git(&git_long, ["branch", "--show-current"])
    );
    assert_eq!(
        run_skron(&skron_long, ["branch", "-r"]),
        git(&git_long, ["branch", "-r"])
    );
    assert_eq!(
        run_skron(&skron_long, ["remote", "-v"]),
        git(&git_long, ["remote", "-v"])
    );
    assert_eq!(
        run_skron(&skron_long, ["config", "--get", "branch.feature.remote"]),
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
    run_skron(
        dir.path(),
        [
            "clone",
            "--origin=upstream",
            "--branch=feature",
            "--config=core.autocrlf=input",
            "--single-branch",
            source.to_str().expect("source path"),
            "skron-equals-options",
        ],
    );
    let git_equals = dir.path().join("git-equals-options");
    let skron_equals = dir.path().join("skron-equals-options");
    assert_eq!(
        run_skron(&skron_equals, ["branch", "--show-current"]),
        git(&git_equals, ["branch", "--show-current"])
    );
    assert_eq!(
        run_skron(&skron_equals, ["branch", "-r"]),
        git(&git_equals, ["branch", "-r"])
    );
    assert_eq!(
        run_skron(&skron_equals, ["config", "--get", "branch.feature.remote"]),
        git(&git_equals, ["config", "--get", "branch.feature.remote"])
    );
    assert_eq!(
        run_skron(&skron_equals, ["config", "--get", "core.autocrlf"]),
        git(&git_equals, ["config", "--get", "core.autocrlf"])
    );

    for (args, git_name, skron_name) in [
        (
            ["--no-checkout", "--checkout"].as_slice(),
            "git-checkout-last",
            "skron-checkout-last",
        ),
        (
            ["--checkout", "--no-checkout"].as_slice(),
            "git-no-checkout-last",
            "skron-no-checkout-last",
        ),
    ] {
        let mut git_clone_args = vec!["clone"];
        git_clone_args.extend_from_slice(args);
        git_clone_args.extend_from_slice(&[source.to_str().expect("source path"), git_name]);
        let mut skron_args = vec!["clone"];
        skron_args.extend_from_slice(args);
        skron_args.extend_from_slice(&[source.to_str().expect("source path"), skron_name]);
        git_args(dir.path(), &git_clone_args);
        run_skron_args(dir.path(), &skron_args);

        let git_clone = dir.path().join(git_name);
        let skron_clone = dir.path().join(skron_name);
        assert_eq!(
            visible_worktree_files(&skron_clone),
            visible_worktree_files(&git_clone),
            "checkout order worktree mismatch for {args:?}"
        );
        assert_eq!(
            skron_clone.join(".git/index").exists(),
            git_clone.join(".git/index").exists(),
            "checkout order index mismatch for {args:?}"
        );
        assert_eq!(
            run_skron(&skron_clone, ["status", "--porcelain=v1", "--branch"]),
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

fn run_skron<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    run_skron_args(cwd, &args)
}

fn run_skron_args(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new(skron_bin())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run skron");
    assert!(
        output.status.success(),
        "skron failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("skron stdout utf8")
        .trim_end_matches('\n')
        .to_owned()
}

fn run_skron_status<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> i32 {
    run_skron_status_args(cwd, &args)
}

fn run_skron_status_args(cwd: &std::path::Path, args: &[&str]) -> i32 {
    Command::new(skron_bin())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run skron")
        .status
        .code()
        .expect("skron exited by signal")
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
    let output = Command::new(command)
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

fn git_with_env<const N: usize>(cwd: &std::path::Path, args: [&str; N]) -> String {
    let output = Command::new("git")
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
    let output = Command::new("git")
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
