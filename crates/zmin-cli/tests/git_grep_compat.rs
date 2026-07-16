mod common;

use std::fs;

use tempfile::TempDir;

use common::{
    command_output_with_env, configure_identity, git, git_args, git_init, git_status,
    git_status_args, git_with_env, run_zmin, run_zmin_args, run_zmin_status, run_zmin_status_args,
    zmin_bin,
};

#[test]
fn grep_matches_stock_git_for_tracked_text_files() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(repo.path().join("a.txt"), b"hello\nworld\nhello world\n").expect("write a");
    fs::write(repo.path().join("dir/b.txt"), b"nested hello\n").expect("write b");
    fs::write(repo.path().join("literal.txt"), b"hello.world\n").expect("write literal");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    assert_eq!(
        run_zmin(repo.path(), ["grep", "hello"]),
        git(repo.path(), ["grep", "hello"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["grep", "-n", "hello"]),
        git(repo.path(), ["grep", "-n", "hello"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["grep", "-l", "hello"]),
        git(repo.path(), ["grep", "-l", "hello"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["grep", "hello", "dir"]),
        git(repo.path(), ["grep", "hello", "dir"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["grep", "-F", "hello.world"]),
        git(repo.path(), ["grep", "-F", "hello.world"])
    );

    fs::write(repo.path().join("a.txt"), b"cached hello\n").expect("write cached grep");
    git(repo.path(), ["add", "a.txt"]);
    fs::write(repo.path().join("a.txt"), b"worktree hello\n").expect("write worktree grep");
    assert_eq!(
        run_zmin(repo.path(), ["grep", "hello", "a.txt"]),
        git(repo.path(), ["grep", "hello", "a.txt"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["grep", "--cached", "hello", "a.txt"]),
        git(repo.path(), ["grep", "--cached", "hello", "a.txt"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["grep", "hello", "HEAD", "--", "a.txt"]),
        git(repo.path(), ["grep", "hello", "HEAD", "--", "a.txt"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["grep", "-n", "hello", "HEAD"]),
        git(repo.path(), ["grep", "-n", "hello", "HEAD"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["grep", "-l", "hello", "HEAD"]),
        git(repo.path(), ["grep", "-l", "hello", "HEAD"])
    );
    assert_eq!(
        run_zmin(repo.path(), ["grep", "hello", "HEAD", "--", "dir"]),
        git(repo.path(), ["grep", "hello", "HEAD", "--", "dir"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["grep", "absent"]),
        git_status(repo.path(), ["grep", "absent"])
    );
}

#[test]
fn grep_documented_local_option_batch_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(
        repo.path().join("a.txt"),
        b"Hello\nworld\nHELLO\nhello.world\nextra hello\n",
    )
    .expect("write a");
    fs::write(
        repo.path().join("b.txt"),
        b"alpha\nhello beta\ngamma\nhello\n",
    )
    .expect("write b");
    fs::write(repo.path().join("dir/c.txt"), b"nested hello\nline two\n").expect("write c");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    for args in [
        ["grep", "-i", "hello"].as_slice(),
        ["grep", "--ignore-case", "hello"].as_slice(),
        ["grep", "-v", "hello"].as_slice(),
        ["grep", "--invert-match", "hello"].as_slice(),
        ["grep", "-c", "hello"].as_slice(),
        ["grep", "--count", "hello"].as_slice(),
        ["grep", "-L", "absent"].as_slice(),
        ["grep", "--files-without-match", "absent"].as_slice(),
        ["grep", "-m", "1", "hello"].as_slice(),
        ["grep", "-m", "1", "-c", "hello"].as_slice(),
        ["grep", "--max-count", "1", "hello"].as_slice(),
        ["grep", "--max-count", "1", "-c", "hello"].as_slice(),
        ["grep", "-H", "hello"].as_slice(),
        ["grep", "--heading", "hello"].as_slice(),
        ["grep", "--break", "hello"].as_slice(),
        ["grep", "--heading", "--break", "hello"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
    }

    assert_eq!(
        run_zmin_status(repo.path(), ["grep", "-L", "hello"]),
        git_status(repo.path(), ["grep", "-L", "hello"])
    );
    assert_eq!(
        run_zmin_status(repo.path(), ["grep", "-c", "absent"]),
        git_status(repo.path(), ["grep", "-c", "absent"])
    );

    let subdir = repo.path().join("dir");
    for args in [
        ["grep", "hello"].as_slice(),
        ["grep", "--full-name", "hello"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(&subdir, args),
            git_args(&subdir, args),
            "subdir args: {args:?}"
        );
    }
}

#[test]
fn grep_regex_filename_and_output_shape_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(
        repo.path().join("a.txt"),
        b"Hello\nworld\nHELLO\nhello.world\nextra hello\n",
    )
    .expect("write a");
    fs::write(
        repo.path().join("b.txt"),
        b"alpha\nhello beta\ngamma\nhello\n",
    )
    .expect("write b");
    fs::write(repo.path().join("dir/c.txt"), b"nested hello\nline two\n").expect("write c");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    for args in [
        ["grep", "--name-only", "hello"].as_slice(),
        ["grep", "-q", "hello"].as_slice(),
        ["grep", "--quiet", "hello"].as_slice(),
        ["grep", "-G", "hello"].as_slice(),
        ["grep", "--basic-regexp", "hello"].as_slice(),
        ["grep", "-E", "hello|world"].as_slice(),
        ["grep", "--extended-regexp", "hello|world"].as_slice(),
        ["grep", "-z", "-l", "hello"].as_slice(),
        ["grep", "--null", "-l", "hello"].as_slice(),
        ["grep", "-a", "hello"].as_slice(),
        ["grep", "--text", "hello"].as_slice(),
        ["grep", "--no-textconv", "hello"].as_slice(),
        ["grep", "--column", "hello"].as_slice(),
        ["grep", "-o", "hello"].as_slice(),
        ["grep", "--only-matching", "hello"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), args),
            git_status_args(repo.path(), args),
            "status args: {args:?}"
        );
    }

    for args in [
        ["grep", "-q", "absent"].as_slice(),
        ["grep", "--quiet", "absent"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_status_args(repo.path(), args),
            git_status_args(repo.path(), args),
            "status args: {args:?}"
        );
    }
}

#[test]
fn grep_context_expression_and_pattern_source_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    fs::write(
        repo.path().join("a.txt"),
        b"alpha\nhello\nbeta\nhello world\ngamma\n",
    )
    .expect("write a");
    fs::write(repo.path().join("b.txt"), b"hello beta\nzeta\n").expect("write b");
    fs::write(repo.path().join("dir/c.txt"), b"outer\nhello_inner\n").expect("write c");
    fs::write(repo.path().join("patterns.txt"), b"hello\nzeta\n").expect("write patterns");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    for args in [
        ["grep", "-A", "1", "hello"].as_slice(),
        ["grep", "--after-context", "1", "hello"].as_slice(),
        ["grep", "-B", "1", "hello"].as_slice(),
        ["grep", "--before-context", "1", "hello"].as_slice(),
        ["grep", "-C", "1", "hello"].as_slice(),
        ["grep", "--context", "1", "hello"].as_slice(),
        ["grep", "-h", "hello"].as_slice(),
        ["grep", "-w", "hello"].as_slice(),
        ["grep", "--word-regexp", "hello"].as_slice(),
        ["grep", "-e", "hello", "--or", "-e", "zeta"].as_slice(),
        ["grep", "-e", "hello", "--and", "-e", "beta"].as_slice(),
        ["grep", "-e", "hello", "--and", "--not", "-e", "world"].as_slice(),
        ["grep", "-f", "patterns.txt"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), args),
            git_status_args(repo.path(), args),
            "status args: {args:?}"
        );
    }
}

#[test]
fn grep_color_all_match_and_function_context_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(
        repo.path().join("sample.c"),
        b"static int alpha(void) {\n    int hello = 1;\n    return hello;\n}\n\nstatic int beta(void) {\n    int zeta = 2;\n    int hello_beta = zeta;\n    return hello_beta;\n}\n",
    )
    .expect("write c sample");
    fs::write(repo.path().join("notes.txt"), b"hello\nzeta\n").expect("write notes");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    for args in [
        ["grep", "--color=always", "hello"].as_slice(),
        ["grep", "--color", "hello"].as_slice(),
        ["grep", "--color=always", "--no-color", "hello"].as_slice(),
        ["grep", "--all-match", "-e", "hello", "--or", "-e", "zeta"].as_slice(),
        ["grep", "-p", "hello"].as_slice(),
        ["grep", "--show-function", "hello"].as_slice(),
        ["grep", "-W", "hello"].as_slice(),
        ["grep", "--function-context", "hello"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), args),
            git_status_args(repo.path(), args),
            "status args: {args:?}"
        );
    }

    let repo_no_all_match = git_init();
    configure_identity(repo_no_all_match.path());
    fs::write(repo_no_all_match.path().join("a.txt"), b"hello\n").expect("write a");
    fs::write(repo_no_all_match.path().join("b.txt"), b"zeta\n").expect("write b");
    git(repo_no_all_match.path(), ["add", "-A"]);
    git_with_env(repo_no_all_match.path(), ["commit", "-m", "initial"]);
    let args = ["grep", "--all-match", "-e", "hello", "--or", "-e", "zeta"];
    assert_eq!(
        run_zmin_status(repo_no_all_match.path(), args),
        git_status(repo_no_all_match.path(), args)
    );
}

#[test]
fn grep_traversal_untracked_and_no_index_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir/sub")).expect("create dir tree");
    fs::write(repo.path().join(".gitignore"), b"*.log\n").expect("write gitignore");
    fs::write(repo.path().join("tracked.txt"), b"hello tracked\n").expect("write tracked");
    fs::write(repo.path().join("dir/sub/tracked2.txt"), b"hello nested\n").expect("write tracked2");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    fs::write(repo.path().join("untracked.txt"), b"hello untracked\n").expect("write untracked");
    fs::write(repo.path().join("ignored.log"), b"hello ignored\n").expect("write ignored");

    for args in [
        ["grep", "--untracked", "hello"].as_slice(),
        ["grep", "--untracked", "--exclude-standard", "hello"].as_slice(),
        ["grep", "--recursive", "hello"].as_slice(),
        ["grep", "--no-recursive", "hello"].as_slice(),
        ["grep", "--max-depth=0", "hello"].as_slice(),
        ["grep", "--max-depth=2", "hello"].as_slice(),
        ["grep", "--threads=1", "hello"].as_slice(),
        ["grep", "--threads=0", "hello"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), args),
            git_status_args(repo.path(), args),
            "status args: {args:?}"
        );
    }

    let conflict_args = ["grep", "--cached", "--untracked", "hello"];
    assert_eq!(
        run_zmin_status(repo.path(), conflict_args),
        git_status(repo.path(), conflict_args)
    );

    let outside = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(outside.path().join("dir/sub")).expect("create outside tree");
    fs::write(outside.path().join("top.txt"), b"hello top\n").expect("write top");
    fs::write(outside.path().join("dir/sub/nested.txt"), b"hello nested\n").expect("write nested");

    for args in [
        ["grep", "--no-index", "hello"].as_slice(),
        ["grep", "--no-index", "--no-recursive", "hello"].as_slice(),
        ["grep", "--no-index", "--max-depth=0", "hello"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(outside.path(), args),
            git_args(outside.path(), args),
            "outside args: {args:?}"
        );
        assert_eq!(
            run_zmin_status_args(outside.path(), args),
            git_status_args(outside.path(), args),
            "outside status args: {args:?}"
        );
    }
}

#[test]
fn grep_schema_tail_and_override_value_family_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::create_dir_all(repo.path().join("dir/sub")).expect("create dir tree");
    fs::write(repo.path().join(".gitignore"), b"*.log\n").expect("write gitignore");
    fs::write(repo.path().join("a.txt"), b"alpha\nhello\nworld\nomega\n").expect("write a");
    fs::write(repo.path().join("b.txt"), b"hello.world\n").expect("write b");
    fs::write(repo.path().join("dir/sub/c.txt"), b"nested hello\n").expect("write c");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);
    fs::write(repo.path().join("u.txt"), b"hello untracked\n").expect("write untracked");
    fs::write(repo.path().join("ignored.log"), b"hello ignored\n").expect("write ignored");

    for args in [
        ["grep", "-r", "hello"].as_slice(),
        ["grep", "--no-exclude-standard", "--untracked", "hello"].as_slice(),
        ["grep", "--textconv", "hello"].as_slice(),
        ["grep", "-P", "hello"].as_slice(),
        ["grep", "--perl-regexp", "hello"].as_slice(),
        ["grep", "-A", "1", "--after-context", "2", "hello"].as_slice(),
        ["grep", "--after-context", "1", "-A", "2", "hello"].as_slice(),
        ["grep", "-B", "1", "--before-context", "2", "hello"].as_slice(),
        ["grep", "--before-context", "1", "-B", "2", "hello"].as_slice(),
        ["grep", "-C", "1", "--context", "2", "hello"].as_slice(),
        ["grep", "--context", "1", "-C", "2", "hello"].as_slice(),
        ["grep", "--max-depth=2", "--max-depth=0", "hello"].as_slice(),
        ["grep", "--max-depth=0", "--max-depth=2", "hello"].as_slice(),
        ["grep", "--threads=1", "--threads=0", "hello"].as_slice(),
        ["grep", "--threads=0", "--threads=1", "hello"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), args),
            git_status_args(repo.path(), args),
            "status args: {args:?}"
        );
    }

    let outside = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(outside.path().join("sub")).expect("create outside tree");
    fs::write(outside.path().join("top.txt"), b"hello top\n").expect("write top");
    fs::write(outside.path().join("sub/nested.txt"), b"hello nested\n").expect("write nested");

    for args in [
        ["grep", "--no-index", "--recursive", "hello"].as_slice(),
        [
            "grep",
            "--no-index",
            "--max-depth=2",
            "--max-depth=0",
            "hello",
        ]
        .as_slice(),
        [
            "grep",
            "--no-index",
            "--max-depth=0",
            "--max-depth=2",
            "hello",
        ]
        .as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(outside.path(), args),
            git_args(outside.path(), args),
            "outside args: {args:?}"
        );
        assert_eq!(
            run_zmin_status_args(outside.path(), args),
            git_status_args(outside.path(), args),
            "outside status args: {args:?}"
        );
    }
}

#[test]
fn grep_binary_submodule_and_pager_schema_tail_matches_stock_git() {
    let repo = git_init();
    configure_identity(repo.path());
    fs::write(repo.path().join("a.txt"), b"hello\n").expect("write text");
    fs::write(repo.path().join("b.bin"), b"hello\0world\n").expect("write binary");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "initial"]);

    for args in [
        ["grep", "-I", "hello"].as_slice(),
        ["grep", "--recurse-submodules", "hello"].as_slice(),
    ] {
        assert_eq!(
            run_zmin_args(repo.path(), args),
            git_args(repo.path(), args),
            "args: {args:?}"
        );
        assert_eq!(
            run_zmin_status_args(repo.path(), args),
            git_status_args(repo.path(), args),
            "status args: {args:?}"
        );
    }

    for args in [
        ["grep", "-O", "hello"].as_slice(),
        ["grep", "--open-files-in-pager", "hello"].as_slice(),
        ["grep", "--open-files-in-pager=less", "hello"].as_slice(),
    ] {
        let zmin = command_output_with_env(
            zmin_bin(),
            repo.path(),
            args,
            &[("GIT_PAGER", "cat")],
            "zmin",
        );
        let git = command_output_with_env("git", repo.path(), args, &[("GIT_PAGER", "cat")], "git");
        assert_eq!(zmin, git, "env args: {args:?}");
    }
}

#[test]
fn grep_cached_lazy_fetch_honors_partial_clone_filter_and_promisor_quiet() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let client = dir.path().join("client");
    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("base.txt"), b"base\n").expect("write base");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "base"]);

    let source_url = format!("file://{}", source.display());
    run_zmin(
        dir.path(),
        [
            "clone",
            "--filter=blob:none",
            "--no-checkout",
            &source_url,
            client.to_str().expect("client path"),
        ],
    );
    run_zmin(&client, ["config", "promisor.quiet", "true"]);

    fs::write(source.join("new.txt"), b"world\n").expect("write new blob");
    git(&source, ["add", "new.txt"]);
    git_with_env(&source, ["commit", "-m", "new blob"]);
    run_zmin(&client, ["fetch", "origin"]);
    run_zmin(&client, ["reset", "--mixed", "origin/main"]);

    let trace_path = dir.path().join("trace.json");
    let (_, stdout, _) = command_output_with_env(
        zmin_bin(),
        &client,
        &["grep", "--cached", "world"],
        &[("GIT_TRACE2_EVENT", trace_path.to_str().expect("trace path"))],
        "zmin lazy grep",
    );
    assert_eq!(stdout, "new.txt:world");
    let trace = fs::read_to_string(trace_path).expect("read trace");
    let fetch_line = trace
        .lines()
        .find(|line| line.contains("fetch.negotiationAlgorithm=noop"))
        .expect("promisor fetch trace");
    assert!(fetch_line.contains("--quiet"), "trace line: {fetch_line}");
}
