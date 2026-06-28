mod common;

use std::fs;
use std::path::Path;

use common::{
    command_any_output, configure_identity, git, git_init, git_with_env, git_with_stdin,
    run_zmin, run_zmin_with_stdin, write_file, zmin_bin,
};

fn seed_fast_export_repo() -> tempfile::TempDir {
    let repo = git_init();
    configure_identity(repo.path());
    git(repo.path(), ["checkout", "-b", "main"]);
    write_file(repo.path(), "a.txt", "one\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "one"]);
    fs::create_dir_all(repo.path().join("dir")).expect("create dir");
    write_file(repo.path(), "dir/b.txt", "two\n");
    git(repo.path(), ["add", "-A"]);
    git_with_env(repo.path(), ["commit", "-m", "two"]);
    repo
}

fn assert_fast_export_matches_stock_git<F>(args: &[&str], prepare: F, expected_files: &[&str])
where
    F: FnOnce(&Path, &Path),
{
    let git_repo = seed_fast_export_repo();
    let zmin_repo = seed_fast_export_repo();
    prepare(git_repo.path(), zmin_repo.path());

    assert_eq!(
        command_any_output("git", git_repo.path(), args, "git fast-export"),
        command_any_output(zmin_bin(), zmin_repo.path(), args, "zmin fast-export")
    );
    for path in expected_files {
        assert_eq!(
            fs::read_to_string(git_repo.path().join(path)).expect("read git file"),
            fs::read_to_string(zmin_repo.path().join(path)).expect("read zmin file")
        );
    }
}

#[test]
fn fast_export_stream_imports_into_stock_git() {
    let source = git_init();
    configure_identity(source.path());
    git(source.path(), ["checkout", "-b", "main"]);
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    fs::create_dir_all(source.path().join("dir")).expect("create dir");
    write_file(source.path(), "dir/b.txt", "two\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);

    let stream = run_zmin(source.path(), ["fast-export", "--all"]);
    let imported = git_init();
    git_with_stdin(imported.path(), ["fast-import"], &stream);

    assert_eq!(
        git(imported.path(), ["log", "--all", "--format=%s"]),
        git(source.path(), ["log", "--all", "--format=%s"])
    );
    assert_eq!(
        git(imported.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
        git(source.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );
    assert_eq!(
        git(
            imported.path(),
            ["cat-file", "-p", "refs/heads/main:dir/b.txt"]
        ),
        git(
            source.path(),
            ["cat-file", "-p", "refs/heads/main:dir/b.txt"]
        )
    );
}

#[test]
fn fast_import_reads_stock_fast_export_stream() {
    let source = git_init();
    configure_identity(source.path());
    git(source.path(), ["checkout", "-b", "main"]);
    write_file(source.path(), "a.txt", "one\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "one"]);
    fs::create_dir_all(source.path().join("dir")).expect("create dir");
    write_file(source.path(), "dir/b.txt", "two\n");
    git(source.path(), ["add", "-A"]);
    git_with_env(source.path(), ["commit", "-m", "two"]);

    let stream = git(source.path(), ["fast-export", "--all"]);
    let imported = git_init();
    run_zmin_with_stdin(imported.path(), ["fast-import"], &stream);

    assert_eq!(
        git(imported.path(), ["log", "--all", "--format=%s"]),
        git(source.path(), ["log", "--all", "--format=%s"])
    );
    assert_eq!(
        git(imported.path(), ["cat-file", "-p", "refs/heads/main:a.txt"]),
        git(source.path(), ["cat-file", "-p", "refs/heads/main:a.txt"])
    );
    assert_eq!(
        git(
            imported.path(),
            ["cat-file", "-p", "refs/heads/main:dir/b.txt"]
        ),
        git(
            source.path(),
            ["cat-file", "-p", "refs/heads/main:dir/b.txt"]
        )
    );
}

#[test]
fn fast_import_reads_bulk_commit_helper_stream_shape() {
    let imported = git_init();
    configure_identity(imported.path());
    git(imported.path(), ["checkout", "-b", "main"]);
    write_file(imported.path(), "base.txt", "base\n");
    git(imported.path(), ["add", "-A"]);
    git_with_env(imported.path(), ["commit", "-m", "base"]);

    let stream = "\
commit HEAD
author A U Thor <author@example.com> 1112912593 -0700
committer C O Mitter <committer@example.com> 1112912593 -0700
data <<EOF
commit 1
EOF
from HEAD^0
M 644 inline 1.t
data <<EOF
content 1
EOF

commit HEAD
author A U Thor <author@example.com> 1112912653 -0700
committer C O Mitter <committer@example.com> 1112912653 -0700
data <<EOF
commit 2
EOF
M 644 inline 2.t
data <<EOF
content 2
EOF

";
    run_zmin_with_stdin(imported.path(), ["fast-import"], stream);

    assert_eq!(
        git(imported.path(), ["log", "--format=%s"]),
        "commit 2\ncommit 1\nbase"
    );
    assert_eq!(
        git(imported.path(), ["rev-parse", "HEAD~2^{commit}"]).len(),
        40
    );
    assert_eq!(
        git(imported.path(), ["cat-file", "-p", "HEAD:2.t"]),
        "content 2"
    );
}

#[test]
fn fast_import_accepts_now_date_format_and_missing_author() {
    let imported = git_init();
    let stream = "\
commit refs/heads/main
mark :1
committer Author <a@uth.or> now
data <<EOF
start
EOF
M 100644 inline file
data <<EOF
contents
EOF
";
    run_zmin_with_stdin(
        imported.path(),
        ["fast-import", "--date-format=now"],
        stream,
    );

    assert_eq!(
        git(
            imported.path(),
            ["log", "--format=%an <%ae>|%cn <%ce>", "main"]
        ),
        "Author <a@uth.or>|Author <a@uth.or>"
    );
    assert_eq!(
        git(imported.path(), ["cat-file", "-p", "main:file"]),
        "contents"
    );
}

#[test]
fn fast_import_accepts_adjacent_commit_records_without_blank_separator() {
    let imported = git_init();
    let stream = "\
commit refs/heads/main
mark :1
committer Author <a@uth.or> now
data <<EOF
start
EOF
M 100644 inline file
data <<EOF
contents
EOF
commit refs/heads/main
committer Author <a@uth.or> now
data <<EOF
tip
EOF
from :1
";
    run_zmin_with_stdin(
        imported.path(),
        ["fast-import", "--date-format=now"],
        stream,
    );

    assert_eq!(
        git(imported.path(), ["log", "--format=%s", "main"]),
        "tip\nstart"
    );
    assert_eq!(
        git(imported.path(), ["cat-file", "-p", "main:file"]),
        "contents"
    );
}

#[test]
fn fast_export_documented_option_batch_matches_stock_git() {
    for args in [
        &["fast-export", "--fake-missing-tagger", "--all"][..],
        &["fast-export", "--signed-tags=warn", "--all"],
        &["fast-export", "--tag-of-filtered-object=drop", "--all"],
        &["fast-export", "--reencode=yes", "--all"],
        &["fast-export", "--reference-excluded-parents", "--all"],
        &["fast-export", "--mark-tags", "--all"],
        &["fast-export", "-M", "--all"],
        &["fast-export", "-C", "--all"],
        &["fast-export", "--refspec=refs/heads/main:refs/heads/main", "--all"],
        &["fast-export", "--import-marks-if-exists=missing.marks", "--all"],
        &["fast-export", "--full-tree", "--all"],
        &["fast-export", "--show-original-ids", "--all"],
        &["fast-export", "--use-done-feature", "--all"],
        &["fast-export", "--no-data", "--all"],
        &["fast-export", "--progress=1", "--all"],
    ] {
        assert_fast_export_matches_stock_git(args, |_, _| {}, &[]);
    }

    assert_fast_export_matches_stock_git(
        &["fast-export", "--export-marks=marks.txt", "--all"],
        |_, _| {},
        &["marks.txt"],
    );
    assert_fast_export_matches_stock_git(
        &["fast-export", "--import-marks=marks.txt", "--all"],
        |git_repo, zmin_repo| {
            let _ = command_any_output(
                "git",
                git_repo,
                &["fast-export", "--export-marks=marks.txt", "--all"],
                "git fast-export export-marks seed",
            );
            let _ = command_any_output(
                "git",
                zmin_repo,
                &["fast-export", "--export-marks=marks.txt", "--all"],
                "git fast-export export-marks seed",
            );
        },
        &[],
    );
}

#[test]
fn fast_export_anonymize_option_family_matches_stock_git() {
    assert_fast_export_matches_stock_git(&["fast-export", "--anonymize", "--all"], |_, _| {}, &[]);
    assert_fast_export_matches_stock_git(
        &["fast-export", "--anonymize", "--anonymize-map=map.txt", "--all"],
        |git_repo, zmin_repo| {
            fs::write(git_repo.join("map.txt"), "seed\n").expect("write git anonymize map");
            fs::write(zmin_repo.join("map.txt"), "seed\n").expect("write zmin anonymize map");
        },
        &[],
    );
}

#[test]
fn fast_export_anonymize_map_requires_anonymize_like_stock_git() {
    let repo = seed_fast_export_repo();
    fs::write(repo.path().join("map.txt"), "seed\n").expect("write anonymize map");
    assert_eq!(
        command_any_output(
            "git",
            repo.path(),
            &["fast-export", "--anonymize-map=map.txt", "--all"],
            "git fast-export anonymize-map precondition",
        ),
        command_any_output(
            zmin_bin(),
            repo.path(),
            &["fast-export", "--anonymize-map=map.txt", "--all"],
            "zmin fast-export anonymize-map precondition",
        )
    );
}

#[test]
fn fast_export_documented_value_families_match_stock_git() {
    for args in [
        &["fast-export", "--signed-tags=abort", "--all"][..],
        &["fast-export", "--signed-tags=verbatim", "--all"],
        &["fast-export", "--signed-tags=warn-strip", "--all"],
        &["fast-export", "--signed-tags=strip", "--all"],
        &["fast-export", "--tag-of-filtered-object=abort", "--all"],
        &["fast-export", "--tag-of-filtered-object=rewrite", "--all"],
        &["fast-export", "--reencode=no", "--all"],
        &["fast-export", "--reencode=abort", "--all"],
        &["fast-export", "--progress=2", "--all"],
        &["fast-export", "--use-done-feature", "--progress=2", "--all"],
        &["fast-export", "--no-data", "--show-original-ids", "--all"],
        &["fast-export", "--full-tree", "--show-original-ids", "--all"],
    ] {
        assert_fast_export_matches_stock_git(args, |_, _| {}, &[]);
    }
}

#[test]
fn fast_export_selection_progress_and_marks_combinations_match_stock_git() {
    for args in [
        &["fast-export", "main"][..],
        &["fast-export", "refs/heads/main"],
        &["fast-export", "--all", "HEAD"],
        &["fast-export", "--all", "refs/heads/main"],
        &["fast-export", "--progress=0", "--all"],
        &["fast-export", "--progress=3", "--all"],
        &["fast-export", "--use-done-feature", "--progress=0", "--all"],
    ] {
        assert_fast_export_matches_stock_git(args, |_, _| {}, &[]);
    }

    for args in [
        &[
            "fast-export",
            "--import-marks-if-exists=missing.marks",
            "--export-marks=marks.txt",
            "--all",
        ][..],
        &[
            "fast-export",
            "--export-marks=marks.txt",
            "--import-marks-if-exists=missing.marks",
            "--all",
        ],
    ] {
        assert_fast_export_matches_stock_git(args, |_, _| {}, &["marks.txt"]);
    }
}

#[test]
fn fast_export_invalid_value_diagnostics_match_stock_git() {
    let repo = seed_fast_export_repo();

    for args in [
        &["fast-export", "--progress=bogus", "--all"][..],
        &["fast-export", "--signed-tags=bogus", "--all"],
        &["fast-export", "--tag-of-filtered-object=bogus", "--all"],
        &["fast-export", "--reencode=bogus", "--all"],
    ] {
        assert_eq!(
            command_any_output("git", repo.path(), args, "git fast-export invalid value"),
            command_any_output(zmin_bin(), repo.path(), args, "zmin fast-export invalid value")
        );
    }
}

#[test]
fn fast_export_anonymize_map_token_forms_match_stock_git() {
    for args in [
        &["fast-export", "--anonymize", "--anonymize-map=foo", "--all"][..],
        &["fast-export", "--anonymize", "--anonymize-map=foo:bar", "--all"],
        &[
            "fast-export",
            "--anonymize",
            "--anonymize-map=foo",
            "--anonymize-map=bar:baz",
            "--all",
        ],
    ] {
        assert_fast_export_matches_stock_git(args, |_, _| {}, &[]);
    }
}

#[test]
fn fast_export_duplicate_and_mixed_positional_selection_match_stock_git() {
    for args in [
        &["fast-export", "HEAD", "HEAD"][..],
        &["fast-export", "main", "main"],
        &["fast-export", "refs/heads/main", "refs/heads/main"],
        &["fast-export", "HEAD", "main"],
        &["fast-export", "main", "HEAD"],
        &["fast-export", "HEAD", "refs/heads/main"],
        &["fast-export", "refs/heads/main", "HEAD"],
        &["fast-export", "main", "refs/heads/main"],
        &["fast-export", "refs/heads/main", "main"],
    ] {
        assert_fast_export_matches_stock_git(args, |_, _| {}, &[]);
    }
}
