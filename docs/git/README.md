# Git Core Overview

This repository implements a Git-compatible core in Rust and exposes it as reusable primitives instead of a single monolithic executable.

## Layers

- `skron-git-core`: canonical Git data structures and repository operations.
- `skron-primitives`: runtime contracts and adapter seams.
- `skron-cli`: command surface built on top of the same primitives.
- `skron-git-remote-http`: dedicated HTTP helper for remote flows.

## Repository compatibility

The repository path stays in canonical Git form:

- stock refs and symbolic refs
- loose objects
- packfiles and pack indexes
- index format
- reflogs
- worktree files

That is the basis for bidirectional handoff between stock Git and Skron.

## Extensibility

The extension points live around the repository, not inside the repository format itself.

- `GitHashAlgorithm` controls object hashing at the Git layer.
- `id::generate()` gives separate opaque ids for higher-level metadata.
- `GitPrimitiveRuntime` lets custom clients swap transport, object, refs, worktree, patch, and rewrite adapters.

This is the model to use when you need Git interoperability and still want room for custom runtime behavior.
