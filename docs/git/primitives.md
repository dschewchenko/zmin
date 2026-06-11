# Public Primitives

The public API is intentionally split into a few narrow seams.

## `skron-git-core`

Repository-format primitives:

- objects and object hashing
- trees and commits
- refs and packed refs
- index read/write
- checkout and merge-file
- diff and reachability
- loose object and pack handling

## `skron-primitives`

Reusable outer seams:

- `git_runtime::GitTransport`
- `git_runtime::GitObjectStore`
- `git_runtime::GitRefsStore`
- `git_runtime::GitWorktreeEngine`
- `git_runtime::GitPatchRenderer`
- `git_runtime::GitRewriteEngine`
- `git_runtime::GitPrimitiveRuntime`

These are the interfaces to use when embedding the core into another client or service.

## IDs and hashing

There are two separate knobs:

- repository object hashing through `skron_git_core::GitHashAlgorithm`
- higher-level opaque ids through `skron_primitives::id`

That separation is deliberate. Git compatibility stays anchored in canonical repository state, while surrounding metadata can evolve independently.
