use std::collections::BTreeMap;
use std::io::{self, Read};

use zmin_primitives::git_runtime::{
    CustomClientPrimitiveRuntime, GitEndpoint, GitObjectEnvelope, GitOid, GitPrimitiveRuntime,
    GitPrimitiveRuntimeFactory, GitRefName, GitRuntimeMode, GitTransport, GitTransportService,
    RefDiscovery, RefUpdate, RefUpdateAction, TransferRequest,
};
use zmin_primitives::{Error, Result};

#[derive(Clone, Debug)]
struct ProbeTransport {
    remote: GitEndpoint,
    refs: BTreeMap<GitRefName, GitOid>,
}

impl GitTransport for ProbeTransport {
    fn discover_refs(&self, remote: &GitEndpoint) -> Result<RefDiscovery> {
        if remote != &self.remote {
            return Err(Error::Validation {
                details: format!("unsupported remote {remote}"),
            });
        }

        Ok(RefDiscovery {
            refs: self.refs.clone(),
            symref: Some((String::from("HEAD"), String::from("refs/heads/main"))),
        })
    }

    fn has_remote_object(&self, remote: &GitEndpoint, oid: &GitOid) -> Result<bool> {
        if remote != &self.remote {
            return Err(Error::Validation {
                details: format!("unsupported remote {remote}"),
            });
        }
        if oid.is_empty() {
            return Err(Error::Validation {
                details: String::from("invalid oid"),
            });
        }
        Ok(true)
    }

    fn upload_pack_request(
        &self,
        request: &TransferRequest,
        stdin: &mut dyn Read,
    ) -> Result<Vec<u8>> {
        if request.remote != self.remote {
            return Err(Error::Validation {
                details: String::from("unexpected remote"),
            });
        }

        let mut buffer = Vec::new();
        stdin
            .read_to_end(&mut buffer)
            .map_err(|error| Error::Storage {
                details: format!("read upload stdin: {error}"),
            })?;

        let kind = match request.service {
            GitTransportService::UploadPack => "upload-pack",
            GitTransportService::ReceivePack => "receive-pack",
        };
        Ok(format!("ok:{kind}:{}B", buffer.len()).into_bytes())
    }

    fn receive_pack_request(
        &self,
        request: &TransferRequest,
        stdin: &mut dyn Read,
    ) -> Result<Vec<u8>> {
        self.upload_pack_request(request, stdin)
    }

    fn read_object_stream(
        &self,
        remote: &GitEndpoint,
        oid: &GitOid,
        writer: &mut dyn io::Write,
    ) -> Result<usize> {
        if remote != &self.remote {
            return Err(Error::Validation {
                details: String::from("unexpected remote"),
            });
        }
        if oid.is_empty() {
            return Err(Error::Validation {
                details: String::from("invalid oid"),
            });
        }

        let payload = format!("fetched:{oid}");
        writer
            .write_all(payload.as_bytes())
            .map_err(|error| Error::Storage {
                details: format!("write stream: {error}"),
            })?;
        Ok(payload.len())
    }
}

#[test]
fn custom_client_primitive_runtime_smoke_is_mode_stable() {
    let remote = String::from("https://example.test/repo.git");
    let base_transport = ProbeTransport {
        remote: remote.clone(),
        refs: BTreeMap::from([
            (String::from("HEAD"), String::from("refs/heads/main")),
            (
                String::from("refs/heads/main"),
                String::from("0".repeat(40)),
            ),
            (String::from("refs/tags/v1"), String::from("1".repeat(40))),
        ]),
    };

    for mode in [
        GitRuntimeMode::Public,
        GitRuntimeMode::Private,
        GitRuntimeMode::Secret,
    ] {
        let runtime = CustomClientPrimitiveRuntime::new_with_mode(
            mode,
            ProbeTransport {
                remote: base_transport.remote.clone(),
                refs: base_transport.refs.clone(),
            },
        );

        assert_eq!(runtime.runtime_mode(), mode);

        let discovered = runtime
            .transport()
            .discover_refs(&remote)
            .expect("discover remote");
        assert_eq!(
            discovered.refs.get("refs/heads/main"),
            Some(&"0".repeat(40))
        );

        let blob = runtime
            .objects()
            .write_object_content(
                &GitObjectEnvelope {
                    id: "2".repeat(40),
                    size: 3,
                    object_type: String::from("blob"),
                    metadata: BTreeMap::new(),
                },
                b"ok\n",
            )
            .expect("write object");
        assert_eq!(
            runtime
                .objects()
                .read_object_content(&blob)
                .expect("read object"),
            b"ok\n".to_vec()
        );

        runtime
            .worktree()
            .materialize_file(&String::from("repo/file.txt"), b"content")
            .expect("materialize file");
        assert_eq!(
            runtime
                .worktree()
                .read_path(&String::from("repo/file.txt"))
                .expect("read worktree file"),
            b"content".to_vec()
        );

        runtime
            .refs()
            .write_ref(&String::from("refs/heads/main"), &"0".repeat(40))
            .expect("write ref");
        let refs = runtime
            .refs()
            .list_refs(Some("refs/heads"))
            .expect("list refs");
        assert!(refs.iter().any(|(name, _)| name == "refs/heads/main"));
        let mut visited = Vec::new();
        runtime
            .refs()
            .visit_refs(Some("refs/heads"), &mut |name, oid| {
                visited.push((name.clone(), oid.clone()));
                Ok(())
            })
            .expect("visit refs");
        assert!(visited.iter().any(|(name, _)| name == "refs/heads/main"));

        let mut out = Vec::new();
        let stream_len = runtime
            .transport()
            .read_object_stream(&remote, &blob, &mut out)
            .expect("stream object");
        assert!(stream_len > 0);
        assert!(String::from_utf8_lossy(&out).contains(&blob));

        let mut upload_stdin = io::Cursor::new(b"local pack");
        let upload = runtime
            .transport()
            .upload_pack_request(
                &TransferRequest {
                    remote: remote.clone(),
                    service: GitTransportService::UploadPack,
                    refspecs: vec![String::from("+refs/heads/main:refs/heads/main")],
                    atomic: false,
                    thin_pack: false,
                    depth: None,
                    filter: None,
                    lease: None,
                },
                &mut upload_stdin,
            )
            .expect("upload request");
        assert!(String::from_utf8_lossy(&upload).starts_with("ok:upload-pack"));

        let err = runtime
            .transport()
            .discover_refs(&String::from("https://example.test/missing.git"))
            .expect_err("must fail");
        assert!(matches!(err, Error::Validation { .. }));

        assert!(
            runtime
                .refs()
                .write_ref(&String::from("refs/heads/main"), &"x".repeat(40))
                .is_ok()
        );

        let patch_render_err = runtime
            .patch_renderer()
            .render_format_patch(&["a".repeat(40)], &mut Vec::new(), false)
            .expect_err("missing commit must fail");
        assert!(
            matches!(
                patch_render_err,
                Error::Validation { .. } | Error::Storage { .. }
            ),
            "{patch_render_err:?}"
        );
    }
}

#[test]
fn custom_client_primitive_runtime_keeps_reference_updates_consistent() {
    let runtime = CustomClientPrimitiveRuntime::new(ProbeTransport {
        remote: String::from("origin"),
        refs: BTreeMap::from([]),
    });

    runtime
        .refs()
        .write_ref(&String::from("refs/heads/main"), &"0".repeat(40))
        .expect("seed ref");
    let updates = [zmin_primitives::git_runtime::RefUpdate {
        name: String::from("refs/heads/main"),
        old_oid: Some("0".repeat(40)),
        new_oid: Some("1".repeat(40)),
        action: RefUpdateAction::Update,
        reason: None,
    }];
    runtime.refs().begin_transaction().expect("begin tx");
    runtime
        .refs()
        .apply_ref_updates(&updates)
        .expect("apply updates");
    runtime.refs().commit_transaction().expect("commit tx");
    assert_eq!(
        runtime
            .refs()
            .read_ref(&String::from("refs/heads/main"))
            .expect("read updated"),
        Some("1".repeat(40))
    );

    let bad = runtime
        .refs()
        .apply_ref_updates(&[RefUpdate {
            name: String::from("refs/heads/main"),
            old_oid: Some("x".repeat(40)),
            new_oid: Some("2".repeat(40)),
            action: RefUpdateAction::NoChange,
            reason: None,
        }])
        .err()
        .expect("bad update must fail");

    assert!(matches!(bad, Error::Validation { .. }));
}

#[test]
fn custom_client_primitive_runtime_patch_and_rewrite_paths_are_mode_stable() {
    let remote = String::from("https://example.test/repo.git");
    let transport = ProbeTransport {
        remote,
        refs: BTreeMap::new(),
    };

    let mut first_format_patch = None;
    let mut first_diff_patch = None;
    let mut first_replacement = None;
    let mut first_retree = None;

    for mode in [
        GitRuntimeMode::Public,
        GitRuntimeMode::Private,
        GitRuntimeMode::Secret,
    ] {
        let runtime = CustomClientPrimitiveRuntime::new_with_mode(mode, transport.clone());

        let parent_tree = write_object(runtime.objects(), "4", "tree", b"parent tree payload");
        let head_tree = write_object(runtime.objects(), "5", "tree", b"head tree payload");
        let parent_commit =
            write_object(runtime.objects(), "6", "commit", b"parent commit payload");
        let head_commit = write_object(runtime.objects(), "7", "commit", b"head commit payload");

        let mut format_patch_out = Vec::new();
        let format_patch_len = runtime
            .patch_renderer()
            .render_format_patch(
                std::slice::from_ref(&head_commit),
                &mut format_patch_out,
                false,
            )
            .expect("render format patch");
        assert_eq!(format_patch_len, format_patch_out.len());
        let format_patch_text = String::from_utf8(format_patch_out).expect("utf8 patch");
        assert!(format_patch_text.contains("Subject: [PATCH 1]"));
        assert!(format_patch_text.contains(&head_commit));
        match first_format_patch {
            None => first_format_patch = Some(format_patch_text),
            Some(ref expected) => assert_eq!(expected, &format_patch_text),
        }

        let mut diff_patch_out = Vec::new();
        let diff_patch_len = runtime
            .patch_renderer()
            .render_diff_patch(&parent_tree, &head_tree, &mut diff_patch_out)
            .expect("render diff patch");
        assert_eq!(diff_patch_len, diff_patch_out.len());
        let diff_patch_text = String::from_utf8(diff_patch_out).expect("utf8 diff");
        assert!(diff_patch_text.contains("diff --git"));
        assert!(diff_patch_text.contains(&parent_tree[..7]));
        assert!(diff_patch_text.contains(&head_tree[..7]));
        match first_diff_patch {
            None => first_diff_patch = Some(diff_patch_text),
            Some(ref expected) => assert_eq!(expected, &diff_patch_text),
        }

        let replacement_id = runtime
            .rewrite()
            .create_replacement_commit(&head_commit, "replacement message")
            .expect("rewrite replacement commit");
        let replacement_bytes = runtime
            .objects()
            .read_object_content(&replacement_id)
            .expect("read replacement bytes");
        let replacement_text = String::from_utf8(replacement_bytes).expect("utf8 replacement");
        assert!(replacement_text.contains(&head_commit));
        assert!(replacement_text.contains("replacement message"));
        match first_replacement {
            None => first_replacement = Some(replacement_text),
            Some(ref expected) => assert_eq!(expected, &replacement_text),
        }

        let retree_id = runtime
            .rewrite()
            .rewrite_commit_tree(&parent_commit, &head_tree)
            .expect("rewrite commit tree");
        let retree_bytes = runtime
            .objects()
            .read_object_content(&retree_id)
            .expect("read retree bytes");
        let retree_text = String::from_utf8(retree_bytes).expect("utf8 retree");
        assert!(retree_text.contains(&parent_commit));
        assert!(retree_text.contains(&head_tree));
        match first_retree {
            None => first_retree = Some(retree_text),
            Some(ref expected) => assert_eq!(expected, &retree_text),
        }
    }
}

#[test]
fn custom_client_primitive_runtime_factory_preserves_mode_and_failure_shape() {
    let remote = String::from("https://example.test/repo.git");
    let transport = ProbeTransport {
        remote: remote.clone(),
        refs: BTreeMap::from([(
            String::from("refs/heads/main"),
            String::from("0".repeat(40)),
        )]),
    };

    for mode in [
        GitRuntimeMode::Public,
        GitRuntimeMode::Private,
        GitRuntimeMode::Secret,
    ] {
        let runtime = GitPrimitiveRuntimeFactory::custom_client_with_transport(
            mode,
            std::sync::Arc::new(transport.clone()),
        );

        assert_eq!(runtime.runtime_mode(), mode);

        assert!(
            runtime
                .transport()
                .discover_refs(&remote)
                .map(|discovery| discovery.symref.is_some())
                .expect("must discover remote")
        );

        let err = runtime
            .transport()
            .discover_refs(&String::from("https://example.test/missing.git"))
            .expect_err("missing remote must error");
        assert!(matches!(err, Error::Validation { .. }));

        let write_result = runtime
            .refs()
            .write_ref(&String::from("refs/heads/main"), &"0".repeat(40));
        assert!(write_result.is_ok());
    }
}

fn write_object(
    objects: &dyn zmin_primitives::git_runtime::GitObjectStore,
    seed: &str,
    object_type: &str,
    content: &[u8],
) -> GitOid {
    objects
        .write_object_content(
            &GitObjectEnvelope {
                id: seed.repeat(40),
                size: content.len(),
                object_type: object_type.to_owned(),
                metadata: BTreeMap::new(),
            },
            content,
        )
        .expect("write object")
}
