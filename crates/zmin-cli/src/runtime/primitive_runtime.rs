use zmin_primitives::git_runtime::{
    GitObjectEnvelope, GitObjectStore, GitPatchRenderer, GitPrimitiveRuntime, GitRefsStore,
    GitRewriteEngine, GitRuntimeMode, GitTransport, GitWorktreeEngine, RefUpdate, RefUpdateAction,
};
use zmin_primitives::{Error as PrimitiveError, Result as PrimitiveResult};

use zmin_git_core::GitHashAlgorithm;

use super::{
    CliPatchRenderer, CliRewriteEngine, CliTransportAdapter, CliWorktreeAdapter, GitRepo,
    OwnedCliObjectStoreAdapter, OwnedCliRefsStoreAdapter, read_common_git_dir,
};

#[derive(Debug)]
pub(crate) struct CliPrimitiveRuntime {
    mode: GitRuntimeMode,
    object_store: OwnedCliObjectStoreAdapter,
    refs_store: OwnedCliRefsStoreAdapter,
    transport: CliTransportAdapter,
    worktree: CliWorktreeAdapter,
    patch_renderer: CliPatchRenderer,
    rewrite: CliRewriteEngine,
}

impl CliPrimitiveRuntime {
    pub(crate) fn new_from_repo(repo: &GitRepo, algorithm: GitHashAlgorithm) -> Self {
        Self::new_from_repo_with_mode(repo, algorithm, GitRuntimeMode::Public)
    }

    pub(crate) fn new_from_repo_with_mode(
        repo: &GitRepo,
        algorithm: GitHashAlgorithm,
        mode: GitRuntimeMode,
    ) -> Self {
        let object_store = OwnedCliObjectStoreAdapter::from_path(&repo.objects_dir, algorithm);
        Self {
            mode,
            object_store: object_store.clone(),
            refs_store: OwnedCliRefsStoreAdapter::from_paths(
                &repo.git_dir,
                read_common_git_dir(&repo.git_dir)
                    .expect("valid repository common git dir for primitive runtime"),
                algorithm,
            ),
            transport: CliTransportAdapter,
            worktree: CliWorktreeAdapter::new(repo),
            patch_renderer: CliPatchRenderer::new(repo.clone(), object_store.clone()),
            rewrite: CliRewriteEngine::new(object_store),
        }
    }

    pub(crate) fn new_default(repo: &GitRepo) -> Self {
        Self::new_from_repo(repo, GitHashAlgorithm::Sha1)
    }

    pub(crate) fn object_store_adapter(&self) -> &OwnedCliObjectStoreAdapter {
        &self.object_store
    }

    pub(crate) fn refs_store_adapter(&self) -> &OwnedCliRefsStoreAdapter {
        &self.refs_store
    }
}

impl GitPrimitiveRuntime for CliPrimitiveRuntime {
    fn transport(&self) -> &dyn GitTransport {
        &self.transport
    }

    fn objects(&self) -> &dyn GitObjectStore {
        &self.object_store
    }

    fn refs(&self) -> &dyn GitRefsStore {
        &self.refs_store
    }

    fn worktree(&self) -> &dyn GitWorktreeEngine {
        &self.worktree
    }

    fn patch_renderer(&self) -> &dyn GitPatchRenderer {
        &self.patch_renderer
    }

    fn rewrite(&self) -> &dyn GitRewriteEngine {
        &self.rewrite
    }

    fn runtime_mode(&self) -> GitRuntimeMode {
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::Cursor;
    use tempfile::TempDir;
    use zmin_git_core::{
        GitHashAlgorithm, GitObjectKind, GitObjectSink, LooseObjectStore, ObjectId, RefStore,
    };
    use zmin_primitives::git_runtime::GitOid;

    fn register_transport_services_for_tests() {
        crate::cli::commands::register_runtime_services();
    }

    #[test]
    fn cli_primitive_runtime_exposes_object_and_refs_adapters() {
        let temp = TempDir::new().expect("temp dir");
        let repo = GitRepo {
            root: temp.path().to_path_buf(),
            git_dir: temp.path().join(".git"),
            objects_dir: temp.path().join(".git/objects"),
            index_path: temp.path().join(".git/index"),
        };
        let runtime = CliPrimitiveRuntime::new_default(&repo);

        assert_eq!(runtime.runtime_mode(), GitRuntimeMode::Public);

        let object_id = runtime
            .objects()
            .write_object_content(
                &GitObjectEnvelope {
                    id: "0".repeat(40),
                    size: 0,
                    object_type: "blob".to_owned(),
                    metadata: Default::default(),
                },
                b"abc",
            )
            .expect("write object");

        let envelope = runtime
            .objects()
            .read_envelope(&object_id, None)
            .expect("read envelope");
        assert_eq!(envelope.object_type, "blob");

        runtime
            .refs()
            .write_ref(&"refs/heads/main".to_owned(), &object_id)
            .expect("write ref");

        let current = runtime
            .refs()
            .read_ref(&"refs/heads/main".to_owned())
            .expect("read ref")
            .expect("ref value");
        assert_eq!(current, object_id);

        assert!(
            runtime
                .transport()
                .discover_refs(&"file:///tmp/remote".to_owned())
                .is_err()
        );
    }

    #[test]
    fn owned_object_and_refs_adapters_keep_read_write_contracts() {
        let temp = TempDir::new().expect("temp dir");
        let git_dir = temp.path().join(".git");
        let objects_dir = git_dir.join("objects");
        let object_store =
            OwnedCliObjectStoreAdapter::from_path(&objects_dir, GitHashAlgorithm::Sha1);
        let refs_store = OwnedCliRefsStoreAdapter::from_path(&git_dir, GitHashAlgorithm::Sha1);

        let id = object_store
            .write_object_content(
                &GitObjectEnvelope {
                    id: "0".repeat(40),
                    size: 0,
                    object_type: "blob".to_owned(),
                    metadata: Default::default(),
                },
                b"hello",
            )
            .expect("write object in owned adapter");

        assert_eq!(
            object_store
                .read_object_content(&id)
                .expect("read object bytes"),
            b"hello"
        );

        refs_store
            .write_ref(&"refs/heads/unit".to_owned(), &id)
            .expect("write owned ref");
        assert_eq!(
            refs_store
                .read_ref(&"refs/heads/unit".to_owned())
                .expect("read owned ref"),
            Some(id)
        );

        let list = refs_store
            .list_refs(Some("refs/heads"))
            .expect("list owned refs");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "refs/heads/unit");
    }

    #[test]
    fn cli_primitive_runtime_supports_all_runtime_modes() {
        let temp = TempDir::new().expect("temp dir");
        let repo = GitRepo {
            root: temp.path().to_path_buf(),
            git_dir: temp.path().join(".git"),
            objects_dir: temp.path().join(".git/objects"),
            index_path: temp.path().join(".git/index"),
        };

        for mode in [
            GitRuntimeMode::Public,
            GitRuntimeMode::Private,
            GitRuntimeMode::Secret,
        ] {
            let runtime =
                CliPrimitiveRuntime::new_from_repo_with_mode(&repo, GitHashAlgorithm::Sha1, mode);
            assert_eq!(runtime.runtime_mode(), mode);

            let object_id = runtime
                .objects()
                .write_object_content(
                    &GitObjectEnvelope {
                        id: "0".repeat(40),
                        size: 0,
                        object_type: "blob".to_owned(),
                        metadata: Default::default(),
                    },
                    b"payload",
                )
                .expect("write object");

            runtime
                .refs()
                .write_ref(&format!("refs/heads/{mode:?}"), &object_id)
                .expect("write mode-specific ref");
            assert_eq!(
                runtime
                    .refs()
                    .read_ref(&format!("refs/heads/{mode:?}"))
                    .expect("read mode ref"),
                Some(object_id)
            );
        }
    }

    #[test]
    fn cli_primitive_runtime_transport_errors_are_mode_independent() {
        let temp = TempDir::new().expect("temp dir");
        let repo = GitRepo {
            root: temp.path().to_path_buf(),
            git_dir: temp.path().join(".git"),
            objects_dir: temp.path().join(".git/objects"),
            index_path: temp.path().join(".git/index"),
        };
        let remote = String::from("file:///tmp/does-not-exist");

        let mut first_error = None;
        for mode in [
            GitRuntimeMode::Public,
            GitRuntimeMode::Private,
            GitRuntimeMode::Secret,
        ] {
            let runtime =
                CliPrimitiveRuntime::new_from_repo_with_mode(&repo, GitHashAlgorithm::Sha1, mode);
            let error = runtime
                .transport()
                .discover_refs(&remote)
                .expect_err("transport must fail on unsupported mode");
            let text = format!("{error:?}");
            match first_error {
                None => first_error = Some(text),
                Some(ref first) => assert_eq!(first, &text),
            }
        }
    }

    #[test]
    fn cli_primitive_runtime_transport_error_shape_is_stable_across_modes() {
        let temp = TempDir::new().expect("temp dir");
        let repo = GitRepo {
            root: temp.path().to_path_buf(),
            git_dir: temp.path().join(".git"),
            objects_dir: temp.path().join(".git/objects"),
            index_path: temp.path().join(".git/index"),
        };
        let remote = String::from("file:///tmp/does-not-exist");

        let mut first: Option<String> = None;
        for mode in [
            GitRuntimeMode::Public,
            GitRuntimeMode::Private,
            GitRuntimeMode::Secret,
        ] {
            let runtime =
                CliPrimitiveRuntime::new_from_repo_with_mode(&repo, GitHashAlgorithm::Sha1, mode);
            let error = runtime
                .transport()
                .discover_refs(&remote)
                .expect_err("discover should fail for invalid local remote");
            let text = format!("{error:?}");
            match first {
                None => first = Some(text),
                Some(ref first_text) => {
                    assert_eq!(first_text, &text, "error shape must match per mode")
                }
            }
        }
    }

    #[test]
    fn cli_primitive_runtime_transport_discover_ref_map_is_shared_across_modes() {
        let repo = init_local_repo_for_transport_tests();
        let mut first: Option<BTreeMap<String, String>> = None;
        for mode in [
            GitRuntimeMode::Public,
            GitRuntimeMode::Private,
            GitRuntimeMode::Secret,
        ] {
            let runtime = CliPrimitiveRuntime::new_from_repo_with_mode(
                &repo.target,
                GitHashAlgorithm::Sha1,
                mode,
            );
            let discovery = runtime
                .transport()
                .discover_refs(&repo.remote_path)
                .expect("discover_refs should work");

            let mut rows = BTreeMap::new();
            rows.insert("HEAD".into(), repo.head_id.clone());
            for (name, value) in discovery.refs {
                rows.insert(name, value);
            }
            match first {
                None => first = Some(rows),
                Some(ref expected) => assert_eq!(expected, &rows),
            }
        }
    }

    #[test]
    fn cli_primitive_runtime_transport_upload_pack_is_mode_stable_for_fetch_path() {
        register_transport_services_for_tests();

        let repo = init_local_repo_for_transport_tests();
        let remote = &repo.remote_path;

        let mut first_pack: Option<Vec<u8>> = None;
        let request = build_fetch_pack_request(&repo.commit_id);
        for mode in [
            GitRuntimeMode::Public,
            GitRuntimeMode::Private,
            GitRuntimeMode::Secret,
        ] {
            let runtime = CliPrimitiveRuntime::new_from_repo_with_mode(
                &repo.target,
                GitHashAlgorithm::Sha1,
                mode,
            );
            let mut input = Cursor::new(request.clone());
            let response = runtime
                .transport()
                .upload_pack_request(
                    &zmin_primitives::git_runtime::TransferRequest {
                        remote: remote.clone(),
                        service: zmin_primitives::git_runtime::GitTransportService::UploadPack,
                        refspecs: vec![],
                        atomic: false,
                        thin_pack: false,
                        depth: None,
                        filter: None,
                        lease: None,
                    },
                    &mut input,
                )
                .expect("upload pack should work");
            assert!(!response.is_empty());
            match first_pack {
                None => first_pack = Some(response),
                Some(ref expected) => assert_eq!(expected, &response),
            }
        }
    }

    #[test]
    fn cli_primitive_runtime_transport_receive_pack_is_mode_stable_for_push_path() {
        register_transport_services_for_tests();

        let mut first_status: Option<Vec<u8>> = None;
        for mode in [
            GitRuntimeMode::Public,
            GitRuntimeMode::Private,
            GitRuntimeMode::Secret,
        ] {
            let repo = init_local_repo_for_transport_tests();
            let remote = &repo.remote_path;
            let update_request =
                build_receive_pack_delete_request(&repo.target_ref, &repo.commit_id);
            let runtime = CliPrimitiveRuntime::new_from_repo_with_mode(
                &repo.target,
                GitHashAlgorithm::Sha1,
                mode,
            );
            let mut input = Cursor::new(update_request.clone());
            let output = runtime
                .transport()
                .receive_pack_request(
                    &zmin_primitives::git_runtime::TransferRequest {
                        remote: remote.clone(),
                        service: zmin_primitives::git_runtime::GitTransportService::ReceivePack,
                        refspecs: vec![],
                        atomic: false,
                        thin_pack: false,
                        depth: None,
                        filter: None,
                        lease: None,
                    },
                    &mut input,
                )
                .expect("receive pack should work");
            assert!(!output.is_empty());
            match first_status {
                None => first_status = Some(output),
                Some(ref expected) => assert_eq!(expected, &output),
            }
        }
    }

    #[test]
    fn cli_primitive_runtime_transport_object_stream_is_mode_stable_for_fetch_read() {
        let repo = init_local_repo_for_transport_tests();
        let remote = &repo.remote_path;
        let mut first: Option<Vec<u8>> = None;
        for mode in [
            GitRuntimeMode::Public,
            GitRuntimeMode::Private,
            GitRuntimeMode::Secret,
        ] {
            let runtime = CliPrimitiveRuntime::new_from_repo_with_mode(
                &repo.target,
                GitHashAlgorithm::Sha1,
                mode,
            );
            let mut out = Vec::new();
            let written = runtime
                .transport()
                .read_object_stream(remote, &repo.payload_id, &mut out)
                .expect("read object stream should work");
            assert_eq!(written, out.len());
            assert_eq!(out, b"transport test object".to_vec());
            match first {
                None => first = Some(out),
                Some(ref expected) => assert_eq!(expected, &out),
            }
        }
    }

    #[test]
    fn cli_primitive_runtime_refs_transaction_is_rollback_stable_on_modes() {
        let temp = TempDir::new().expect("temp dir");
        let git_dir = temp.path().join(".git");
        std::fs::create_dir_all(&git_dir).expect("create git dir");
        let refs_store = OwnedCliRefsStoreAdapter::from_path(&git_dir, GitHashAlgorithm::Sha1);
        refs_store
            .write_ref(&"refs/heads/main".to_string(), &"0".repeat(40))
            .expect("seed ref");

        let object = "1".repeat(40);
        let mut first_failure = None;
        for mode in [
            GitRuntimeMode::Public,
            GitRuntimeMode::Private,
            GitRuntimeMode::Secret,
        ] {
            let runtime = CliPrimitiveRuntime::new_from_repo_with_mode(
                &GitRepo {
                    root: temp.path().to_path_buf(),
                    git_dir: git_dir.clone(),
                    objects_dir: temp.path().join(".git/objects"),
                    index_path: temp.path().join(".git/index"),
                },
                GitHashAlgorithm::Sha1,
                mode,
            );

            runtime
                .refs()
                .begin_transaction()
                .expect("transaction begin");
            let result = runtime.refs().apply_ref_updates(&[RefUpdate {
                name: "refs/heads/main".to_string(),
                old_oid: Some("9".repeat(40)),
                new_oid: Some(object.clone()),
                action: RefUpdateAction::Update,
                reason: Some(String::from("stale update should fail")),
            }]);
            let failure = result.expect_err("expected stale old oid failure");
            let details = format!("{failure:?}");

            match first_failure {
                None => first_failure = Some(details),
                Some(ref expected) => assert_eq!(expected, &details),
            }

            runtime.refs().rollback_transaction().expect("rollback");
            let current = runtime
                .refs()
                .read_ref(&"refs/heads/main".to_string())
                .expect("read after rollback")
                .expect("main exists");
            assert_eq!(current, "0".repeat(40));
        }
    }

    #[test]
    fn cli_primitive_runtime_object_exists_and_read_errors_match_modes() {
        let temp = TempDir::new().expect("temp dir");
        let repo = GitRepo {
            root: temp.path().to_path_buf(),
            git_dir: temp.path().join(".git"),
            objects_dir: temp.path().join(".git/objects"),
            index_path: temp.path().join(".git/index"),
        };
        std::fs::create_dir_all(&repo.objects_dir).expect("create objects dir");
        std::fs::create_dir_all(&repo.git_dir).expect("create git dir");
        let runtime = CliPrimitiveRuntime::new_from_repo_with_mode(
            &repo,
            GitHashAlgorithm::Sha1,
            GitRuntimeMode::Public,
        );

        let id = runtime
            .objects()
            .write_object_content(
                &GitObjectEnvelope {
                    id: "0".repeat(40),
                    size: 4,
                    object_type: String::from("blob"),
                    metadata: Default::default(),
                },
                b"hello",
            )
            .expect("write object");

        assert!(runtime.objects().object_exists(&id).expect("exists true"));

        let invalid = "not-a-valid-oid";
        let mut first_read_error: Option<String> = None;

        for mode in [
            GitRuntimeMode::Public,
            GitRuntimeMode::Private,
            GitRuntimeMode::Secret,
        ] {
            let mode_runtime =
                CliPrimitiveRuntime::new_from_repo_with_mode(&repo, GitHashAlgorithm::Sha1, mode);

            let exists = mode_runtime
                .objects()
                .object_exists(&invalid.to_string())
                .expect("invalid object id should follow object_exists contract");
            assert!(!exists);

            let read_err = mode_runtime
                .objects()
                .read_object_content(&invalid.to_string())
                .expect_err("invalid object read must fail");
            let read_text = format!("{read_err:?}");
            match first_read_error {
                None => first_read_error = Some(read_text),
                Some(ref expected) => assert_eq!(&read_text, expected),
            }
        }
    }

    struct TransportFixture {
        _temp: TempDir,
        target: GitRepo,
        remote_path: String,
        payload_id: String,
        target_ref: String,
        head_id: String,
        commit_id: String,
    }

    struct PrimitiveSurfaceFixture {
        _temp: TempDir,
        repo: GitRepo,
        parent_commit_id: String,
        head_commit_id: String,
        head_tree_id: String,
        parent_tree_id: String,
    }

    fn init_local_repo_for_transport_tests() -> TransportFixture {
        let temp = TempDir::new().expect("temp");
        let repo_root = temp.path().join("source");
        let source_git_dir = repo_root.join(".git");
        std::fs::create_dir_all(source_git_dir.join("objects")).expect("create objects");
        let repo = GitRepo {
            root: repo_root.clone(),
            git_dir: source_git_dir.clone(),
            objects_dir: source_git_dir.join("objects"),
            index_path: source_git_dir.join("index"),
        };

        let store = LooseObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1);
        let tree_id = store
            .write_object(
                GitObjectKind::Tree,
                &zmin_git_core::encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let signature =
            zmin_git_core::Signature::new("A", "a@example.test", 1, "+0000").expect("signature");
        let commit_id = {
            let object = zmin_git_core::CommitBuilder::new(
                tree_id.clone(),
                signature.clone(),
                signature.clone(),
            )
            .message("transport test commit\n")
            .expect("commit message")
            .encode()
            .expect("encode commit");
            store
                .write_object(GitObjectKind::Commit, &object)
                .expect("write transport commit")
                .to_hex()
        };

        let payload = b"transport test object";
        let payload_id = store
            .write_object(GitObjectKind::Blob, payload)
            .expect("write transport object")
            .to_hex();
        let head = ObjectId::new(GitHashAlgorithm::Sha1, &[1_u8; 20]);
        let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
        refs.write_ref("refs/heads/main", &head).expect("write ref");
        refs.write_symbolic_ref("HEAD", "refs/heads/main")
            .expect("write symbolic HEAD");
        refs.write_ref(
            "refs/heads/transport",
            &ObjectId::from_hex(GitHashAlgorithm::Sha1, &commit_id).expect("commit id"),
        )
        .expect("write transport ref");

        TransportFixture {
            _temp: temp,
            target: repo,
            remote_path: repo_root.to_string_lossy().to_string(),
            payload_id,
            head_id: head.to_hex(),
            target_ref: "refs/heads/transport".into(),
            commit_id,
        }
    }

    fn init_repo_for_primitive_surface_tests() -> PrimitiveSurfaceFixture {
        let temp = TempDir::new().expect("temp");
        let repo_root = temp.path().join("primitive");
        let git_dir = repo_root.join(".git");
        let objects_dir = git_dir.join("objects");
        std::fs::create_dir_all(&objects_dir).expect("create objects");
        std::fs::create_dir_all(&repo_root).expect("create repo root");

        let repo = GitRepo {
            root: repo_root.clone(),
            git_dir: git_dir.clone(),
            objects_dir: objects_dir.clone(),
            index_path: git_dir.join("index"),
        };

        let store = LooseObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1);
        let signature =
            zmin_git_core::Signature::new("A", "a@example.test", 1, "+0000").expect("signature");

        let parent_blob = store
            .write_object(GitObjectKind::Blob, b"hello\n")
            .expect("write parent blob");
        let parent_tree_content = zmin_git_core::encode_tree(&[zmin_git_core::TreeEntry {
            mode: zmin_git_core::TreeMode::File,
            name: b"note.txt".to_vec(),
            id: parent_blob,
        }])
        .expect("encode parent tree");
        let parent_tree = store
            .write_object(GitObjectKind::Tree, &parent_tree_content)
            .expect("write parent tree");
        let parent_commit = zmin_git_core::CommitBuilder::new(
            parent_tree.clone(),
            signature.clone(),
            signature.clone(),
        )
        .message("parent commit\n")
        .expect("parent message")
        .encode()
        .expect("encode parent commit");
        let parent_commit_id = store
            .write_object(GitObjectKind::Commit, &parent_commit)
            .expect("write parent commit")
            .to_hex();

        let head_blob = store
            .write_object(GitObjectKind::Blob, b"hello from head\n")
            .expect("write head blob");
        let head_tree_content = zmin_git_core::encode_tree(&[zmin_git_core::TreeEntry {
            mode: zmin_git_core::TreeMode::File,
            name: b"note.txt".to_vec(),
            id: head_blob,
        }])
        .expect("encode head tree");
        let head_tree = store
            .write_object(GitObjectKind::Tree, &head_tree_content)
            .expect("write head tree");
        let head_commit =
            zmin_git_core::CommitBuilder::new(head_tree.clone(), signature.clone(), signature)
                .parent(
                    ObjectId::from_hex(GitHashAlgorithm::Sha1, &parent_commit_id)
                        .expect("parent id"),
                )
                .message("head commit\n")
                .expect("head message")
                .encode()
                .expect("encode head commit");
        let head_commit_id = store
            .write_object(GitObjectKind::Commit, &head_commit)
            .expect("write head commit")
            .to_hex();

        let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
        let head_commit_oid =
            ObjectId::from_hex(GitHashAlgorithm::Sha1, &head_commit_id).expect("head oid");
        refs.write_ref("refs/heads/main", &head_commit_oid)
            .expect("write main ref");
        refs.write_symbolic_ref("HEAD", "refs/heads/main")
            .expect("write HEAD");

        PrimitiveSurfaceFixture {
            _temp: temp,
            repo,
            parent_commit_id,
            head_commit_id,
            head_tree_id: head_tree.to_hex(),
            parent_tree_id: parent_tree.to_hex(),
        }
    }

    #[test]
    fn cli_primitive_runtime_patch_and_worktree_paths_are_mode_stable() {
        let fixture = init_repo_for_primitive_surface_tests();
        let mut first_patch = None;

        for mode in [
            GitRuntimeMode::Public,
            GitRuntimeMode::Private,
            GitRuntimeMode::Secret,
        ] {
            let runtime = CliPrimitiveRuntime::new_from_repo_with_mode(
                &fixture.repo,
                GitHashAlgorithm::Sha1,
                mode,
            );

            runtime
                .worktree()
                .materialize_file(&String::from("mode/proof.txt"), mode.as_str().as_bytes())
                .expect("materialize worktree file");
            assert_eq!(
                runtime
                    .worktree()
                    .read_path(&String::from("mode/proof.txt"))
                    .expect("read materialized file"),
                mode.as_str().as_bytes()
            );
            runtime
                .worktree()
                .rename_path(
                    &String::from("mode/proof.txt"),
                    &String::from("mode/proof-renamed.txt"),
                )
                .expect("rename worktree file");
            runtime
                .worktree()
                .touch_path(&String::from("mode/touched.txt"))
                .expect("touch worktree file");
            runtime
                .worktree()
                .remove_path(&String::from("mode/proof-renamed.txt"))
                .expect("remove worktree file");

            let mut out = Vec::new();
            let written = runtime
                .patch_renderer()
                .render_format_patch(&[fixture.head_commit_id.clone()], &mut out, false)
                .expect("render format patch");
            assert_eq!(written, out.len());
            let text = String::from_utf8(out).expect("utf8 patch");
            assert!(text.contains("Subject: [PATCH] head commit"));
            assert!(text.contains("note.txt"));
            assert!(text.contains("+hello from head"));
            match first_patch {
                None => first_patch = Some(text),
                Some(ref expected) => assert_eq!(expected, &text),
            }

            let mut diff_out = Vec::new();
            let diff_written = runtime
                .patch_renderer()
                .render_diff_patch(
                    &fixture.parent_tree_id,
                    &fixture.head_tree_id,
                    &mut diff_out,
                )
                .expect("render diff patch");
            assert_eq!(diff_written, diff_out.len());
            let diff_text = String::from_utf8(diff_out).expect("utf8 diff");
            assert!(diff_text.contains("diff --git a/note.txt b/note.txt"));
            assert!(diff_text.contains("+hello from head"));
        }
    }

    #[test]
    fn cli_primitive_runtime_rewrite_paths_are_mode_stable() {
        let fixture = init_repo_for_primitive_surface_tests();
        let mut first_rewritten_message = None;
        let mut first_retree_message = None;

        for mode in [
            GitRuntimeMode::Public,
            GitRuntimeMode::Private,
            GitRuntimeMode::Secret,
        ] {
            let runtime = CliPrimitiveRuntime::new_from_repo_with_mode(
                &fixture.repo,
                GitHashAlgorithm::Sha1,
                mode,
            );

            let replacement_id = runtime
                .rewrite()
                .create_replacement_commit(&fixture.head_commit_id, "replacement message\n")
                .expect("rewrite replacement commit");
            let replacement_bytes = runtime
                .objects()
                .read_object_content(&replacement_id)
                .expect("read replacement commit");
            let replacement_commit =
                zmin_git_core::decode_commit(GitHashAlgorithm::Sha1, &replacement_bytes)
                    .expect("decode replacement commit");
            let replacement_message =
                String::from_utf8(replacement_commit.message.clone()).expect("replacement utf8");
            assert_eq!(replacement_message, "replacement message\n");
            assert_eq!(replacement_commit.tree.to_hex(), fixture.head_tree_id);
            match first_rewritten_message {
                None => first_rewritten_message = Some(replacement_message),
                Some(ref expected) => assert_eq!(expected, &replacement_message),
            }

            let retree_id = runtime
                .rewrite()
                .rewrite_commit_tree(&fixture.head_commit_id, &fixture.parent_tree_id)
                .expect("rewrite commit tree");
            let retree_bytes = runtime
                .objects()
                .read_object_content(&retree_id)
                .expect("read retree commit");
            let retree_commit = zmin_git_core::decode_commit(GitHashAlgorithm::Sha1, &retree_bytes)
                .expect("decode retree commit");
            let retree_message =
                String::from_utf8(retree_commit.message.clone()).expect("retree utf8");
            assert_eq!(retree_commit.tree.to_hex(), fixture.parent_tree_id);
            assert_eq!(retree_message, "head commit\n");
            let parent_ids = retree_commit
                .parents
                .iter()
                .map(|parent| parent.to_hex())
                .collect::<Vec<_>>();
            assert_eq!(parent_ids, vec![fixture.parent_commit_id.clone()]);
            match first_retree_message {
                None => first_retree_message = Some(retree_message),
                Some(ref expected) => assert_eq!(expected, &retree_message),
            }
        }
    }

    fn write_pkt_line(out: &mut Vec<u8>, payload: &str) {
        out.extend_from_slice(format!("{:04x}", payload.len() + 4).as_bytes());
        out.extend_from_slice(payload.as_bytes());
    }

    fn build_fetch_pack_request(payload_id: &str) -> Vec<u8> {
        let mut request = Vec::new();
        write_pkt_line(&mut request, &format!("want {payload_id}\n"));
        write_pkt_line(&mut request, "done\n");
        request.extend_from_slice(b"0000");
        request
    }

    fn build_receive_pack_delete_request(ref_name: &str, payload_id: &str) -> Vec<u8> {
        let zero = "0".repeat(40);
        let mut request = Vec::new();
        write_pkt_line(
            &mut request,
            &format!("{payload_id} {zero} {ref_name}\0report-status ofs-delta\n"),
        );
        request.extend_from_slice(b"0000");
        request
    }
}
