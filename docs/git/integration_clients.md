# Client Integration Guide

This repository can be consumed as a library without shelling out to `git`.

## Minimal model

1. Use `skron-git-core` for repository-format operations.
2. Use `skron-primitives::git_runtime` when you need pluggable transport/object/refs/worktree adapters.
3. Keep any custom ids or metadata outside the repository object model.

## Small custom runtime example

```rust
use skron_primitives::git_runtime::{
    GitObjectEnvelope, GitPrimitiveRuntime, GitPrimitiveRuntimeFactory, GitRuntimeMode,
};
use std::collections::BTreeMap;

let runtime = GitPrimitiveRuntimeFactory::in_memory(GitRuntimeMode::Public);
let blob_id = runtime
    .objects()
    .write_object_content(
        &GitObjectEnvelope {
            id: "0".repeat(40),
            size: 2,
            object_type: "blob".into(),
            metadata: BTreeMap::new(),
        },
        b"ok",
    )
    .expect("write");

assert_eq!(runtime.objects().read_object_content(&blob_id).unwrap(), b"ok");
```

## Practical guidance

- If you need stock Git interoperability, keep repository object ids and on-disk structure in Git-compatible mode.
- If you need custom metadata ids, generate them separately with `skron_primitives::id`.
- If you need a different hashing policy for application data, apply it outside the `.git` object graph.
