# Zmin

Zmin is a Git-compatible VCS implemented in Rust, with reusable Git primitives, a daily command-line interface, and room for richer terminal workflows.

The name is a transliteration from Ukrainian: `zmin` comes from `змін`, meaning "of changes". That fits the project directly: Zmin is a system for tracking, comparing, moving, and preserving changes while staying compatible with Git repositories.

## What This Project Is

Zmin is a Rust implementation of Git-oriented repository primitives and workflow building blocks.

The goal is not only to reproduce Git-compatible behavior, but to expose the lower-level pieces as explicit, composable modules that can be reused independently:

- repository and object primitives;
- command and workflow building blocks;
- transport and runtime boundaries;
- replaceable hashing, identifier, storage, and adapter layers.

## How It Differs From Upstream Git

Zmin is not the upstream Git project and not a line-by-line port of the upstream Git codebase.

It keeps Git concepts and compatibility goals while reorganizing the implementation into modular primitives and extension points.

The focus is on:

- compatibility with Git repositories and common Git workflows;
- modularity instead of a monolithic implementation;
- embeddability instead of a standalone tool only;
- replaceable internal components instead of fixed built-in choices.

## What Is Here

- `zmin-git-core`: low-level Git-compatible objects, refs, index, pack, checkout, diff, merge-file, commit, tag, and reachability primitives.
- `zmin-cli`: Git-style CLI and future TUI built directly on top of the core primitives.
- `zmin-primitives`: shared runtime contracts, transport traits, config helpers, ids, and error model.
- `zmin-git-remote-http`: standalone HTTP remote helper binary for Git transport flows.
- `zmin-core`: thin umbrella crate that re-exports the main Git-facing crates.

## Public Extension Points

The public surface is meant to be reused.

- Git object hashing at the repository layer is explicit and selectable through `zmin_git_core::GitHashAlgorithm`.
- Application-level identifiers are separate from repository object ids through `zmin_primitives::id`.
- Transport, refs, objects, worktree, patch rendering, and rewrite seams are exposed through `zmin_primitives::git_runtime`.

That split lets you keep repository state fully Git-compatible while still changing higher-level ids, runtime policies, or metadata hashing outside the `.git` format.

## Compatibility

The current scope targets stock Git compatibility on macOS and Linux.

In practice that means:

- stock Git can continue from repositories written by Zmin;
- Zmin can continue from repositories written by stock Git;
- repository structure, refs, loose objects, packfiles, index state, reflogs, and worktree state stay compatible;
- stock Git validation commands such as `fsck`, `verify-pack`, and related compatibility suites are part of the gate.

Current proof lives in:

- `crates/zmin-cli/tests/`
- `docs/git/parity_evidence_matrix.md`
- `docs/cli/compatibility_acceptance.md`
- `tools/git-cli-readiness-status.sh`

## Performance

In a local MacBook Pro M1 16 GB benchmark, the current Zmin CLI release build measured faster than upstream/Homebrew Git `2.53.0` by `13%` to `93%` across the validated local history, write, pack, clone, fetch, and push operations.

Against Gitoxide `gix 0.54.0`, Zmin matched the median for `log` and `merge-base`, and measured `50%` to `82%` faster on the other comparable CLI-adjacent operations.

Detailed operation-by-operation results and methodology notes are in `docs/cli/performance_benchmark_2026-05-18.md`.

## Small Example

```rust
use zmin_core::git_core::{GitHashAlgorithm, GitObjectKind, GitObjectSink, InMemoryObjectStore};
use zmin_core::git_runtime::{GitObjectEnvelope, GitPrimitiveRuntimeFactory, GitRuntimeMode};
use zmin_core::id::generate;
use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repo_id = generate();

    let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha256);
    let blob_id = store.write_object(GitObjectKind::Blob, b"hello from zmin")?;

    let runtime = GitPrimitiveRuntimeFactory::in_memory(GitRuntimeMode::Public);
    let envelope = GitObjectEnvelope {
        id: "0".repeat(40),
        size: 2,
        object_type: "blob".into(),
        metadata: BTreeMap::new(),
    };
    let runtime_blob = runtime.objects().write_object_content(&envelope, b"ok")?;

    println!("repo_id={repo_id}");
    println!("blob_id={blob_id}");
    println!("runtime_blob={runtime_blob}");
    Ok(())
}
```

## Build

```bash
cargo build
cargo build -p zmin-cli
cargo build -p zmin-git-remote-http
```

## Verify

```bash
cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets --all-features
cargo test --all
tools/git-cli-readiness-status.sh --require-complete
```

## Direction

The core primitives are the stable foundation. Planned work is concentrated on a richer terminal interface and a more approachable CLI built on top of the same Git-compatible engine.

## Status

Active development. The core primitives are intended to remain a stable foundation while the command-line and terminal interfaces continue to expand around them.

## Trademark Notice

Zmin is not affiliated with the Git Project or Software Freedom Conservancy.

Git is a registered trademark of Software Freedom Conservancy, Inc.

## License

This repository is currently shared publicly for evaluation and reference only.

No permission is granted to use, copy, modify, distribute, sublicense, or create derivative works from this code without prior written consent.
