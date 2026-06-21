mod common;

use std::fs;
use std::process::Command;

use tempfile::TempDir;

use common::{
    configure_identity, git, git_args, git_init, git_status, git_with_env, run_zmin_args,
    write_file, zmin_bin,
};

fn command_output_any(
    command: &str,
    cwd: &std::path::Path,
    args: &[&str],
) -> (i32, String, String) {
    let output = Command::new(common::test_command_program(command))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|err| panic!("run {command}: {err}"));
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

fn mergetool_conflict_fixture() -> TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "f.txt", "base\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    git(repo.path(), ["checkout", "-b", "feature"]);
    write_file(repo.path(), "f.txt", "feature\n");
    git_with_env(repo.path(), ["commit", "-am", "feature"]);
    git(repo.path(), ["checkout", "main"]);
    write_file(repo.path(), "f.txt", "main\n");
    git_with_env(repo.path(), ["commit", "-am", "main"]);
    assert_ne!(git_status(repo.path(), ["merge", "feature"]), 0);
    repo
}

#[test]
fn ls_files_modes_match_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write a");
    fs::write(repo.path().join("dir/b.txt"), b"nested\n").expect("write b");
    fs::write(repo.path().join("space name.txt"), b"space\n").expect("write space");
    fs::write(repo.path().join("blocked"), b"tracked\n").expect("write blocked");
    fs::write(
        repo.path().join(".gitattributes"),
        b"*.txt text\nspace name.txt text eol=crlf\n",
    )
    .expect("write attributes");
    fs::write(repo.path().join("bin.dat"), b"bin\0data").expect("write binary");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    for args in [
        ["ls-files"].as_slice(),
        ["ls-files", "--stage"].as_slice(),
        ["ls-files", "--stage", "--abbrev"].as_slice(),
        ["ls-files", "--stage", "--abbrev=12"].as_slice(),
        ["ls-files", "--format=%(objectname) %(path)", "--abbrev=12"].as_slice(),
        ["ls-files", "-z"].as_slice(),
        ["ls-files", "-c", "-z"].as_slice(),
        ["ls-files", "-s", "-z"].as_slice(),
        ["ls-files", "--eol"].as_slice(),
        ["ls-files", "--eol", "-z"].as_slice(),
        ["ls-files", "--eol", "-s", "-t"].as_slice(),
        ["ls-files", "--debug"].as_slice(),
        ["ls-files", "--debug", "-s"].as_slice(),
        ["ls-files", "--debug", "-t"].as_slice(),
        ["ls-files", "--format=%(path)"].as_slice(),
        [
            "ls-files",
            "--format=%(objectmode) %(objectname) %(stage) %(path)",
        ]
        .as_slice(),
        ["ls-files", "-z", "--format=%(path)"].as_slice(),
        ["ls-files", "--format=%% %(path) %x09 end"].as_slice(),
        ["ls-files", "-t"].as_slice(),
        ["ls-files", "-f"].as_slice(),
        ["ls-files", "-f", "--modified", "--deleted"].as_slice(),
        ["ls-files", "--sparse"].as_slice(),
        ["ls-files", "--recurse-submodules"].as_slice(),
        [
            "ls-files",
            "--recurse-submodules",
            "--no-recurse-submodules",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    assert_eq!(
        command_output_any(zmin_bin(), repo.path(), &["ls-files", "--format=%(bad)"]),
        command_output_any("git", repo.path(), &["ls-files", "--format=%(bad)"])
    );

    git(
        repo.path(),
        ["update-index", "--assume-unchanged", "space name.txt"],
    );
    for args in [
        ["ls-files", "-v"].as_slice(),
        ["ls-files", "-s", "-v"].as_slice(),
        ["ls-files", "--debug", "-v"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    git(
        repo.path(),
        ["update-index", "--no-assume-unchanged", "space name.txt"],
    );
    git(
        repo.path(),
        ["update-index", "--skip-worktree", "space name.txt"],
    );
    for args in [
        ["ls-files", "-v"].as_slice(),
        ["ls-files", "-t"].as_slice(),
        ["ls-files", "-s", "-v"].as_slice(),
        ["ls-files", "--debug", "-t"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    git(
        repo.path(),
        ["update-index", "--no-skip-worktree", "space name.txt"],
    );
    let subdir = repo.path().join("dir");
    for args in [
        ["ls-files"].as_slice(),
        ["ls-files", "--full-name"].as_slice(),
        ["ls-files", "--full-name", "-s", "-z"].as_slice(),
        ["ls-files", ":(top)a.txt"].as_slice(),
        ["ls-files", ":/a.txt"].as_slice(),
        ["ls-files", "--full-name", ":(top)a.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&subdir, args),
            git_args(&subdir, args),
            "args: {args:?}"
        );
    }
    for args in [
        ["ls-files", "--error-unmatch", "a.txt"].as_slice(),
        ["ls-files", "--error-unmatch", "a.txt", "missing.txt"].as_slice(),
    ] {
        assert_eq!(
            command_output_any(zmin_bin(), repo.path(), args),
            command_output_any("git", repo.path(), args),
            "args: {args:?}"
        );
    }

    fs::write(repo.path().join("a.txt"), b"changed\n").expect("modify a");
    fs::remove_file(repo.path().join("dir/b.txt")).expect("delete b");
    fs::remove_file(repo.path().join("blocked")).expect("delete blocked file");
    fs::create_dir_all(repo.path().join("blocked")).expect("create blocked dir");
    fs::write(repo.path().join("blocked/other.txt"), b"killed\n").expect("write killed file");
    fs::write(repo.path().join("other.txt"), b"other\n").expect("write other");
    fs::create_dir_all(repo.path().join("other-dir")).expect("create other dir");
    fs::write(repo.path().join("other-dir/file.txt"), b"other dir\n").expect("write other dir");
    fs::create_dir_all(repo.path().join("empty-dir")).expect("create empty dir");
    fs::create_dir_all(repo.path().join("nested-ignore")).expect("create nested ignore dir");
    fs::write(repo.path().join("manual.log"), b"log\n").expect("write manual log");
    fs::write(repo.path().join("manual.tmp"), b"tmp\n").expect("write manual tmp");
    fs::write(repo.path().join("excludes.lst"), b"*.tmp\n").expect("write excludes");
    fs::create_dir_all(repo.path().join("scoped")).expect("create scoped dir");
    fs::write(repo.path().join("scoped/.ignore"), b"scoped.tmp\n").expect("write scoped ignore");
    fs::write(repo.path().join("scoped/scoped.tmp"), b"scoped\n").expect("write scoped tmp");
    fs::write(repo.path().join("scoped.tmp"), b"root scoped\n").expect("write root scoped");
    fs::write(repo.path().join(".gitignore"), b"ignored.txt\n").expect("write ignore");
    fs::write(repo.path().join("ignored.txt"), b"ignored\n").expect("write ignored");
    fs::write(repo.path().join("nested-ignore/.gitignore"), b"deep.tmp\n")
        .expect("write nested gitignore");
    fs::write(repo.path().join("nested-ignore/deep.tmp"), b"deep\n").expect("write deep ignored");
    fs::write(repo.path().join("deep.tmp"), b"root deep\n").expect("write root deep");
    fs::write(repo.path().join(".git/info/exclude"), b"info-only.txt\n")
        .expect("write info exclude");
    fs::write(repo.path().join("info-only.txt"), b"info\n").expect("write info-only");
    fs::write(
        repo.path().join("custom-global.ignore"),
        b"global-only.txt\n",
    )
    .expect("write configured global ignore");
    fs::write(repo.path().join("global-only.txt"), b"global\n").expect("write global-only");
    git(
        repo.path(),
        [
            "config",
            "core.excludesFile",
            repo.path()
                .join("custom-global.ignore")
                .to_str()
                .expect("utf8 ignore path"),
        ],
    );

    for args in [
        ["ls-files", "--modified"].as_slice(),
        ["ls-files", "--deleted"].as_slice(),
        ["ls-files", "--killed"].as_slice(),
        ["ls-files", "--killed", "-t"].as_slice(),
        ["ls-files", "--killed", "-z"].as_slice(),
        ["ls-files", "--killed", "--directory"].as_slice(),
        ["ls-files", "--eol", "--modified"].as_slice(),
        ["ls-files", "--eol", "--deleted"].as_slice(),
        ["ls-files", "--ignored", "--others", "--exclude-standard"].as_slice(),
        ["ls-files", "-i", "-o", "-t", "--exclude-standard"].as_slice(),
        ["ls-files", "-t", "--modified", "--deleted"].as_slice(),
        ["ls-files", "--modified", "--deleted", "--deduplicate"].as_slice(),
        ["ls-files", "--others", "--exclude-standard"].as_slice(),
        ["ls-files", "--others", "-x", "*.log"].as_slice(),
        ["ls-files", "--ignored", "--others", "-x", "*.log"].as_slice(),
        ["ls-files", "--others", "-X", "excludes.lst"].as_slice(),
        ["ls-files", "--ignored", "--others", "-X", "excludes.lst"].as_slice(),
        ["ls-files", "--others", "--exclude-per-directory=.ignore"].as_slice(),
        [
            "ls-files",
            "--ignored",
            "--others",
            "--exclude-per-directory=.ignore",
        ]
        .as_slice(),
        ["ls-files", "--others", "--directory"].as_slice(),
        ["ls-files", "--others", "--directory", "--empty-directory"].as_slice(),
        ["ls-files", "--others", "-z"].as_slice(),
        ["ls-files", "--cached", "--others", "--exclude-standard"].as_slice(),
        ["ls-files", "-c", "-o", "-z", "--exclude-standard"].as_slice(),
        ["ls-files", "--stage", "--others"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
    for args in [
        ["ls-files", "-i"].as_slice(),
        ["ls-files", "-i", "-o"].as_slice(),
        ["ls-files", "--others", "--error-unmatch", "other.txt"].as_slice(),
    ] {
        assert_eq!(
            command_output_any(zmin_bin(), repo.path(), args),
            command_output_any("git", repo.path(), args),
            "args: {args:?}"
        );
    }

    git(repo.path(), ["add", "-f", "ignored.txt"]);
    for args in [
        ["ls-files", "--ignored", "--cached", "--exclude-standard"].as_slice(),
        ["ls-files", "-i", "-c", "-t", "--exclude-standard"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn ls_files_eol_untracked_text_binary_classification_matches_stock_git() {
    let repo = git_init();
    let strt = b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let mut prefix = Vec::new();
    for _ in 0..4 {
        prefix.extend_from_slice(strt);
    }
    for (path, suffix) in [
        ("TeBi_127_S", b"BBB\x01".as_slice()),
        ("TeBi_128_S", b"BBBB\x01".as_slice()),
        ("TeBi_127_E", b"BBB\x1a".as_slice()),
        ("TeBi_E_127", b"\x1a".as_slice()),
        ("TeBi_128_N", b"BBBB\0".as_slice()),
        ("TeBi_128_L", b"BBB\n".as_slice()),
        ("TeBi_127_C", b"BBB\r".as_slice()),
        ("TeBi_126_CL", b"BB\r\n".as_slice()),
        ("TeBi_126_CLC", b"BB\r\n\r".as_slice()),
    ] {
        let mut content = Vec::new();
        if path == "TeBi_E_127" {
            content.extend_from_slice(suffix);
            content.extend_from_slice(&prefix);
            content.extend_from_slice(b"BBB");
        } else {
            content.extend_from_slice(&prefix);
            content.extend_from_slice(suffix);
        }
        fs::write(repo.path().join(path), content).expect("write TeBi fixture");
    }

    let mut zmin = run_zmin_args(repo.path(), &["ls-files", "--eol", "-o"])
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut git = git_args(repo.path(), &["ls-files", "--eol", "-o"])
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    zmin.sort();
    git.sort();
    assert_eq!(zmin, git);
}

#[test]
fn ls_files_eol_bare_eol_attribute_reports_implicit_text_like_stock_git() {
    let git_repo = git_init();
    let zmin_repo = git_init();
    for repo in [git_repo.path(), zmin_repo.path()] {
        fs::write(repo.join(".gitattributes"), b"*.txt eol=lf\n").expect("write attributes");
        fs::write(repo.join("file.txt"), b"one\ntwo\n").expect("write file");
        git(repo, ["add", "-A"]);
    }

    assert_eq!(
        run_zmin_args(zmin_repo.path(), &["ls-files", "--eol", "file.txt"]),
        git_args(git_repo.path(), &["ls-files", "--eol", "file.txt"])
    );
}

#[test]
fn ls_files_eol_text_unset_suppresses_eol_attr_like_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join(".gitattributes"),
        b"*.txt -text eol=crlf\n",
    )
    .expect("write attributes");
    fs::write(repo.path().join("lf.txt"), b"one\ntwo\n").expect("write lf fixture");
    fs::write(repo.path().join("crlf.txt"), b"one\r\ntwo\r\n").expect("write crlf fixture");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    for args in [
        ["ls-files", "--eol", "lf.txt"].as_slice(),
        ["ls-files", "--eol", "crlf.txt"].as_slice(),
        ["ls-files", "--eol", "lf.txt", "crlf.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn ls_files_unmerged_index_matches_stock_git() {
    let repo = mergetool_conflict_fixture();
    let matching_modes = [
        vec!["ls-files"],
        vec!["ls-files", "-c"],
        vec!["ls-files", "-t"],
        vec!["ls-files", "-v"],
        vec!["ls-files", "--stage"],
        vec!["ls-files", "-s", "-t"],
        vec!["ls-files", "-s", "-v"],
        vec!["ls-files", "--unmerged"],
        vec!["ls-files", "-u"],
        vec!["ls-files", "--unmerged", "-t"],
        vec!["ls-files", "--unmerged", "-v"],
        vec!["ls-files", "--unmerged", "-z"],
        vec!["ls-files", "--unmerged", "--deduplicate"],
        vec!["ls-files", "--unmerged", "--full-name", "f.txt"],
    ];
    for args in matching_modes {
        assert_eq!(
            run_zmin_args(repo.path(), &args),
            git_args(repo.path(), &args),
            "args: {args:?}"
        );
    }

    for args in [
        ["ls-files", "--unmerged", "--error-unmatch", "f.txt"].as_slice(),
        ["ls-files", "--unmerged", "--error-unmatch", "missing.txt"].as_slice(),
    ] {
        assert_eq!(
            command_output_any(zmin_bin(), repo.path(), args),
            command_output_any("git", repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn ls_files_with_tree_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("a.txt"), b"a\n").expect("write a");
    fs::write(repo.path().join("dir/b.txt"), b"b\n").expect("write b");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    git(repo.path(), ["rm", "--cached", "dir/b.txt"]);

    let matching_modes = [
        vec!["ls-files", "--with-tree=HEAD"],
        vec!["ls-files", "--with-tree=HEAD", "dir/b.txt"],
        vec![
            "ls-files",
            "--with-tree=HEAD",
            "--error-unmatch",
            "dir/b.txt",
        ],
        vec!["ls-files", "--with-tree=HEAD", "--deduplicate"],
        vec!["ls-files", "--with-tree=HEAD", "-t"],
        vec![
            "ls-files",
            "--with-tree=HEAD",
            "--format=%(objectmode) %(path)",
        ],
    ];
    for args in matching_modes {
        assert_eq!(
            run_zmin_args(repo.path(), &args),
            git_args(repo.path(), &args),
            "args: {args:?}"
        );
    }

    for args in [
        ["ls-files", "--with-tree=HEAD", "-s"].as_slice(),
        ["ls-files", "--with-tree=HEAD", "-u"].as_slice(),
    ] {
        assert_eq!(
            command_output_any(zmin_bin(), repo.path(), args),
            command_output_any("git", repo.path(), args),
            "args: {args:?}"
        );
    }
}

#[test]
fn ls_files_recurse_submodules_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let child = dir.path().join("child");
    git(
        dir.path(),
        ["init", "-b", "main", child.to_str().expect("child path")],
    );
    configure_identity(&child);
    fs::write(child.join("c.txt"), b"child\n").expect("write child");
    git(&child, ["add", "-A"]);
    git_with_env(&child, ["commit", "-m", "child"]);

    let repo = dir.path().join("repo");
    git(
        dir.path(),
        ["init", "-b", "main", repo.to_str().expect("repo path")],
    );
    configure_identity(&repo);
    fs::write(repo.join(".gitignore"), b"*.root\n").expect("write root ignore");
    fs::write(repo.join("ignored.root"), b"ignored\n").expect("write ignored root");
    git(&repo, ["add", "-f", ".gitignore", "ignored.root"]);
    let output = Command::new(common::stock_git_bin())
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            child.to_str().expect("child path"),
            "deps/child",
        ])
        .current_dir(&repo)
        .output()
        .expect("submodule add");
    assert!(
        output.status.success(),
        "submodule add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_with_env(&repo, ["commit", "-m", "super"]);

    for args in [
        ["ls-files", "--recurse-submodules"].as_slice(),
        ["ls-files", "--recurse-submodules", "-t"].as_slice(),
        ["ls-files", "--recurse-submodules", "-s"].as_slice(),
        ["ls-files", "--recurse-submodules", "--format=%(path)"].as_slice(),
        [
            "ls-files",
            "--recurse-submodules",
            "--ignored",
            "--cached",
            "--exclude-standard",
        ]
        .as_slice(),
        [
            "ls-files",
            "--recurse-submodules",
            "--ignored",
            "--cached",
            "--exclude-standard",
            "-s",
        ]
        .as_slice(),
        [
            "ls-files",
            "--recurse-submodules",
            "--ignored",
            "--cached",
            "--exclude-standard",
            "-t",
        ]
        .as_slice(),
        [
            "ls-files",
            "--recurse-submodules",
            "--ignored",
            "--cached",
            "--exclude-standard",
            "-z",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&repo, args),
            git_args(&repo, args),
            "args: {args:?}"
        );
    }

    let zmin = command_output_any(
        zmin_bin(),
        &repo,
        &["ls-files", "--recurse-submodules", "-o"],
    );
    let git = command_output_any("git", &repo, &["ls-files", "--recurse-submodules", "-o"]);
    assert_eq!(zmin.0, git.0);
    assert_eq!(zmin.2.lines().next(), git.2.lines().next());

    assert_eq!(
        command_output_any(
            zmin_bin(),
            &repo,
            &["ls-files", "--recurse-submodules", "--modified"],
        ),
        command_output_any(
            "git",
            &repo,
            &["ls-files", "--recurse-submodules", "--modified"],
        )
    );
    assert_eq!(
        command_output_any(
            zmin_bin(),
            &repo,
            &["ls-files", "--recurse-submodules", "--deleted"],
        ),
        command_output_any(
            "git",
            &repo,
            &["ls-files", "--recurse-submodules", "--deleted"],
        )
    );
    assert_eq!(
        command_output_any(
            zmin_bin(),
            &repo,
            &["ls-files", "--recurse-submodules", "--unmerged"],
        ),
        command_output_any(
            "git",
            &repo,
            &["ls-files", "--recurse-submodules", "--unmerged"],
        )
    );
    assert_eq!(
        command_output_any(
            zmin_bin(),
            &repo,
            &["ls-files", "--recurse-submodules", "--resolve-undo"],
        ),
        command_output_any(
            "git",
            &repo,
            &["ls-files", "--recurse-submodules", "--resolve-undo"],
        )
    );
}

#[test]
fn ls_files_resolve_undo_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("f.txt"), b"base\n").expect("write base");
    git(repo.path(), ["add", "f.txt"]);
    git_with_env(repo.path(), ["commit", "-m", "base"]);
    let base_branch = git(repo.path(), ["symbolic-ref", "--short", "HEAD"]);
    git(repo.path(), ["checkout", "-b", "left"]);
    fs::write(repo.path().join("f.txt"), b"left\n").expect("write left");
    git_with_env(repo.path(), ["commit", "-am", "left"]);
    git(repo.path(), ["checkout", &base_branch]);
    git(repo.path(), ["checkout", "-b", "right"]);
    fs::write(repo.path().join("f.txt"), b"right\n").expect("write right");
    git_with_env(repo.path(), ["commit", "-am", "right"]);
    assert_ne!(
        command_output_any("git", repo.path(), &["merge", "left"]).0,
        0
    );
    fs::write(repo.path().join("f.txt"), b"resolved\n").expect("write resolved");
    git(repo.path(), ["add", "f.txt"]);

    for args in [
        ["ls-files", "--resolve-undo"].as_slice(),
        ["ls-files", "--resolve-undo", "-t"].as_slice(),
        ["ls-files", "--resolve-undo", "-v"].as_slice(),
        ["ls-files", "--resolve-undo", "--abbrev=12"].as_slice(),
        ["ls-files", "--resolve-undo", "-z"].as_slice(),
        ["ls-files", "--resolve-undo", "--error-unmatch", "f.txt"].as_slice(),
        ["ls-files", "--resolve-undo", "-s"].as_slice(),
        ["ls-files", "--resolve-undo", "-u"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    let args = ["ls-files", "--resolve-undo", "--format=%(path)"];
    let zmin = command_output_any(zmin_bin(), repo.path(), &args);
    let git = command_output_any("git", repo.path(), &args);
    assert_eq!(zmin.0, git.0);
    assert_eq!(zmin.2.lines().next(), git.2.lines().next());
}
