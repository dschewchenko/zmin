# Reftable Compatibility Plan

This file scopes the remaining `clone --ref-format=reftable` compatibility row
in the current Git contract. Zmin now has partial reader, writer, and
multi-table stack paths, but the row must stay open until those paths are
validated end-to-end against current Git. A config-only implementation is not
compatible.

## Current Evidence

`tools/git-clone-reftable-schema-gap-probe.sh` records the current gap:

- stock Git exits `0` for `git clone --ref-format=reftable <path> dst`
- stock Git reports `reftable` from `rev-parse --show-ref-format`
- stock Git writes `.git/reftable/tables.list` and at least one binary
  `.git/reftable/*.ref` table
- stock Git does not write `.git/refs/heads/main`
- Zmin's reader, writer, and stack implementation is partial; the clone,
  normal ref-resolution, fsck, and platform acceptance rows remain unverified
  by this probe

## Compatibility Contract

The implementation must support the row as real storage, not as display-only
metadata:

- initialize a repository with `extensions.refStorage=reftable` and
  `core.repositoryformatversion=1`
- write clone refs to `.git/reftable`, including `HEAD`,
  `refs/heads/<branch>`, `refs/remotes/<remote>/HEAD`,
  `refs/remotes/<remote>/<branch>` and copied tags where the selected clone
  mode requires them
- avoid loose branch refs for the reftable clone path
- read refs from the same reftable store through normal ref resolution paths
- keep existing files-backed ref behavior unchanged for default and
  `--ref-format=files` clones
- preserve stock clone stdout, stderr, exit code, checked-out worktree, index,
  branch tracking config and remote config

## Non-Solutions

These must not close the row:

- writing only `extensions.refStorage=reftable`
- creating an empty `.git/reftable` directory
- deleting loose refs after checkout while leaving Zmin unable to resolve the
  cloned branch from reftable storage
- using `/usr/bin/git` as an implementation dependency
- marking the row closed with a probe that checks only
  `rev-parse --show-ref-format`

## Verification

Before changing `docs/cli/matrices/clone_v2_47.tsv` to `closed`, extend and
pass `tools/git-clone-reftable-schema-gap-probe.sh` so it compares stock Git
and Zmin for:

- exit code, stdout and stderr
- `rev-parse --show-ref-format`
- `rev-parse HEAD`
- `symbolic-ref HEAD`
- `show-ref --heads --remotes --tags`
- absence of loose `refs/heads/main`
- presence and non-emptiness of `.git/reftable/tables.list`
- clean worktree status and checked-out file contents

Only then should the matrix row move from `open` to `closed`.
