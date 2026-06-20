# Client Integration Guide

This repository can be used as a library without shelling out to `git`.

## Minimal model

1. Use `zmin-git-core` for repository-format operations.
2. Use `zmin-primitives::git_runtime` when you need pluggable transport/object/refs/worktree adapters.
3. Keep any custom ids or metadata outside the repository object model.

## Small custom runtime example

```rust
use zmin_primitives::git_runtime::{
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
- If you need custom metadata ids, generate them separately with `zmin_primitives::id`.
- If you need a different hashing policy for application data, apply it outside the `.git` object graph.

## Local Git Replacement Checks

Preview dogfood uses a local `git` shim that dispatches to `zmin`.

Keep these checks green before asking an IDE or GUI client to use the binary:

```bash
zmin --version
zmin version --build-options
git --version
git status
git fetch --prune --no-tags
```

The version line must start with the Git 2.47 compatibility baseline, currently
`git version 2.47.1.zmin`, and include the real Zmin package version after it. Some
clients reject tools below their minimum Git version before running any other
command.

Run the replacement smoke before local IDE dogfood:

```bash
tools/git-replacement-dogfood-smoke.sh
```

The smoke creates a temporary `git` shim that dispatches to `zmin`, then checks
the IDE-shaped surfaces that usually run first: version, `status -z`,
porcelain v2 branch status, `rev-parse`, `config`, `ls-files -z`, `diff -z`,
`log -z` and `fetch --prune --no-tags`. This is a dogfood gate, not a complete
Git compatibility claim.
