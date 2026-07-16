mod common;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use tempfile::TempDir;

use common::{command_any_output, configure_identity, git, git_with_env, stock_git_bin, zmin_bin};

#[test]
fn fetch_filter_local_file_and_shallow_depth_match_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_local_client = dir.path().join("git-local-client");
    let zmin_local_client = dir.path().join("zmin-local-client");
    let git_file_client = dir.path().join("git-file-client");
    let zmin_file_client = dir.path().join("zmin-file-client");
    let git_shallow_client = dir.path().join("git-shallow-client");
    let zmin_shallow_client = dir.path().join("zmin-shallow-client");

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"one\n").expect("write one");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "one"]);

    git(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            git_local_client.to_str().expect("git local client path"),
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            source.to_str().expect("source path"),
            zmin_local_client.to_str().expect("zmin local client path"),
        ],
    );
    let file_url = format!("file://{}", source.display());
    git(
        dir.path(),
        [
            "clone",
            file_url.as_str(),
            git_file_client.to_str().expect("git file client path"),
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            file_url.as_str(),
            zmin_file_client.to_str().expect("zmin file client path"),
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            file_url.as_str(),
            git_shallow_client
                .to_str()
                .expect("git shallow client path"),
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            file_url.as_str(),
            zmin_shallow_client
                .to_str()
                .expect("zmin shallow client path"),
        ],
    );
    fs::write(source.join("file"), b"two\n").expect("write two");
    git(&source, ["commit", "-am", "two"]);

    assert_filter_fetch_matches_stock(
        &git_local_client,
        &zmin_local_client,
        &["fetch", "--filter=blob:none", "origin", "main"],
    );

    assert_filter_fetch_matches_stock(
        &git_file_client,
        &zmin_file_client,
        &["fetch", "--filter=blob:none", "origin", "main"],
    );

    assert_filter_fetch_matches_stock(
        &git_shallow_client,
        &zmin_shallow_client,
        &["fetch", "--filter=blob:none", "--depth=1", "origin", "main"],
    );
}

#[test]
fn fetch_unshallow_filter_file_remote_matches_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    let file_url = format!("file://{}", source.display());

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"one\n").expect("write one");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "one"]);
    fs::write(source.join("file"), b"two\n").expect("write two");
    git(&source, ["commit", "-am", "two"]);

    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            file_url.as_str(),
            git_client.to_str().expect("git client path"),
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            file_url.as_str(),
            zmin_client.to_str().expect("zmin client path"),
        ],
    );

    assert_filter_fetch_matches_stock(
        &git_client,
        &zmin_client,
        &["fetch", "--unshallow", "--filter=blob:none"],
    );
    assert_eq!(
        git(
            &zmin_client,
            ["config", "--get", "core.repositoryformatversion"]
        ),
        git(
            &git_client,
            ["config", "--get", "core.repositoryformatversion"]
        )
    );
}

#[test]
fn fetch_unshallow_filter_rejects_unknown_extension_like_stock_git() {
    let dir = TempDir::new().expect("temp dir");
    let source = dir.path().join("source");
    let git_client = dir.path().join("git-client");
    let zmin_client = dir.path().join("zmin-client");
    let file_url = format!("file://{}", source.display());

    git(
        dir.path(),
        ["init", "-b", "main", source.to_str().expect("source path")],
    );
    configure_identity(&source);
    fs::write(source.join("file"), b"one\n").expect("write one");
    git(&source, ["add", "-A"]);
    git_with_env(&source, ["commit", "-m", "one"]);
    fs::write(source.join("file"), b"two\n").expect("write two");
    git(&source, ["commit", "-am", "two"]);

    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            file_url.as_str(),
            git_client.to_str().expect("git client path"),
        ],
    );
    git(
        dir.path(),
        [
            "clone",
            "--depth=1",
            file_url.as_str(),
            zmin_client.to_str().expect("zmin client path"),
        ],
    );
    git(&git_client, ["config", "extensions.nonsense", "true"]);
    git(&zmin_client, ["config", "extensions.nonsense", "true"]);

    let args = ["fetch", "--unshallow", "--filter=blob:none"];
    let git_output = command_any_output("git", &git_client, &args, "git");
    let zmin_output = command_any_output(zmin_bin(), &zmin_client, &args, "zmin");
    assert_ne!(git_output.0, 0, "stock git should fail");
    assert_eq!(zmin_output.0, git_output.0);
    assert_eq!(
        git(
            &zmin_client,
            ["config", "--get", "core.repositoryformatversion"]
        ),
        git(
            &git_client,
            ["config", "--get", "core.repositoryformatversion"]
        )
    );
    assert_eq!(
        optional_file_content(&zmin_client.join(".git/shallow")),
        optional_file_content(&git_client.join(".git/shallow"))
    );
}

#[test]
fn partial_clone_promised_missing_reflog_object_fsck_matches_stock_git() {
    fn fixed_git_command(repo: &Path, args: &[&str]) -> (i32, String, String) {
        let output = Command::new(stock_git_bin())
            .args(args)
            .current_dir(repo)
            .env("GIT_AUTHOR_NAME", "Bench")
            .env("GIT_AUTHOR_EMAIL", "bench@example.test")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_NAME", "Bench")
            .env("GIT_COMMITTER_EMAIL", "bench@example.test")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .output()
            .expect("run stock git");
        (
            output.status.code().expect("exit code"),
            String::from_utf8(output.stdout)
                .expect("stdout utf8")
                .trim_end()
                .to_owned(),
            String::from_utf8(output.stderr)
                .expect("stderr utf8")
                .trim_end()
                .to_owned(),
        )
    }

    fn pack_as_from_promisor(repo: &Path, object_id: &str) {
        let mut pack_objects = Command::new(stock_git_bin())
            .args(["pack-objects", ".git/objects/pack/pack"])
            .current_dir(repo)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn pack-objects");
        pack_objects
            .stdin
            .as_mut()
            .expect("pack-objects stdin")
            .write_all(format!("{object_id}\n").as_bytes())
            .expect("write object id");
        let pack_output = pack_objects.wait_with_output().expect("wait pack-objects");
        assert!(
            pack_output.status.success(),
            "pack-objects failed: {}",
            String::from_utf8_lossy(&pack_output.stderr)
        );
        let pack_hash = String::from_utf8(pack_output.stdout)
            .expect("pack hash utf8")
            .trim()
            .to_owned();
        assert!(!pack_hash.is_empty(), "missing pack hash");
        fs::write(
            repo.join(format!(".git/objects/pack/pack-{pack_hash}.promisor")),
            b"",
        )
        .expect("write promisor marker");
    }

    fn delete_object(repo: &Path, object_id: &str) {
        let object_path = repo
            .join(".git/objects")
            .join(&object_id[..2])
            .join(&object_id[2..]);
        fs::remove_file(&object_path).expect("delete loose object");
    }

    fn setup_repo(root: &Path, name: &str) -> PathBuf {
        let repo = root.join(name);
        git(root, ["init", repo.to_str().expect("repo path")]);
        configure_identity(&repo);
        fs::write(repo.join("my_commit.t"), b"my_commit\n").expect("write commit file");
        git(&repo, ["add", "-A"]);
        git_with_env(&repo, ["commit", "-m", "my_commit"]);

        let head_tree = git(&repo, ["rev-parse", "HEAD^{tree}"]);
        let a = fixed_git_command(&repo, &["commit-tree", "-m", "a", head_tree.as_str()]).1;
        let c = fixed_git_command(
            &repo,
            &[
                "commit-tree",
                "-m",
                "c",
                "-p",
                a.as_str(),
                head_tree.as_str(),
            ],
        )
        .1;

        git(&repo, ["branch", "my_branch", a.as_str()]);
        git(&repo, ["branch", "-f", "my_branch", "HEAD"]);
        delete_object(&repo, &a);
        pack_as_from_promisor(&repo, &c);
        repo
    }

    let dir = TempDir::new().expect("temp dir");
    let git_repo = setup_repo(dir.path(), "git-repo");
    let zmin_repo = setup_repo(dir.path(), "zmin-repo");

    let git_before = command_any_output("git", &git_repo, &["fsck"], "git fsck before");
    let zmin_before = command_any_output(zmin_bin(), &zmin_repo, &["fsck"], "zmin fsck before");
    assert_ne!(
        git_before.0, 0,
        "stock git should fail before partial-clone extension"
    );
    assert_ne!(
        zmin_before.0, 0,
        "zmin should fail before partial-clone extension"
    );

    git(&git_repo, ["config", "core.repositoryformatversion", "1"]);
    git(
        &git_repo,
        ["config", "extensions.partialclone", "arbitrary string"],
    );
    git(&zmin_repo, ["config", "core.repositoryformatversion", "1"]);
    git(
        &zmin_repo,
        ["config", "extensions.partialclone", "arbitrary string"],
    );

    let git_after = command_any_output("git", &git_repo, &["fsck"], "git fsck after");
    let zmin_after = command_any_output(zmin_bin(), &zmin_repo, &["fsck"], "zmin fsck after");
    assert_eq!(zmin_after, git_after);
}

fn assert_filter_fetch_matches_stock(git_repo: &Path, zmin_repo: &Path, args: &[&str]) {
    let git_output = command_any_output("git", git_repo, args, "git");
    let zmin_output = command_any_output(zmin_bin(), zmin_repo, args, "zmin");
    assert_eq!(
        zmin_output.0, git_output.0,
        "zmin stderr: {}",
        zmin_output.2
    );
    assert_eq!(zmin_output.1, git_output.1);
    assert_eq!(zmin_output.2, git_output.2);
    assert_eq!(
        fs::read_to_string(zmin_repo.join(".git/FETCH_HEAD")).expect("zmin FETCH_HEAD"),
        fs::read_to_string(git_repo.join(".git/FETCH_HEAD")).expect("git FETCH_HEAD")
    );
    assert_eq!(
        optional_file_content(&zmin_repo.join(".git/shallow")),
        optional_file_content(&git_repo.join(".git/shallow"))
    );
    assert_eq!(
        git(zmin_repo, ["rev-parse", "refs/remotes/origin/main"]),
        git(git_repo, ["rev-parse", "refs/remotes/origin/main"])
    );
    assert_eq!(
        git(
            zmin_repo,
            ["cat-file", "-p", "refs/remotes/origin/main:file"]
        ),
        git(
            git_repo,
            ["cat-file", "-p", "refs/remotes/origin/main:file"]
        )
    );
    assert_eq!(
        git(zmin_repo, ["config", "--get", "remote.origin.promisor"]),
        git(git_repo, ["config", "--get", "remote.origin.promisor"])
    );
    assert_eq!(
        git(
            zmin_repo,
            ["config", "--get", "remote.origin.partialclonefilter"]
        ),
        git(
            git_repo,
            ["config", "--get", "remote.origin.partialclonefilter"]
        )
    );
}

fn optional_file_content(path: &Path) -> Option<String> {
    path.exists()
        .then(|| fs::read_to_string(path).expect("optional file content"))
}
