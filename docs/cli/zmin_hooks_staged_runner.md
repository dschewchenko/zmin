# Zmin Staged Hook Runner

This document defines the Zmin-only staged hook runner. It is outside the Git
`2.47.1` compatibility denominator because stock Git has no `git hooks run`
command and no lint-staged equivalent.

The goal is to replace Husky plus lint-staged style workflows with one
index-backed command that works from normal Git hooks, has predictable file
selection, and is easy to verify.

## Command Shape

Initial implemented surface:

```bash
zmin hooks run <hook> --staged --list
```

Next surfaces, after selector evidence exists:

```bash
zmin hooks run <hook> --staged --ext rs,ts,js --list
zmin hooks run <hook> --staged -- <command> [args...]
zmin hooks run <hook> --staged --ext rs,ts,js -- <command> [args...]
zmin hooks run <hook> --staged --dry-run -- <command> [args...]
```

Do not add parser flags before behavior is implemented and covered. A flag that
parses but does nothing is a product-quality bug, not future-proofing.

## Selection Contract

The staged runner reads the index against `HEAD`. It must not select files from
unstaged working tree changes.

Selected entries:

- added files
- copied files, when copy detection is available
- modified files
- renamed files, using the staged destination path for command execution
- type changes, when the staged destination exists as a file

Deleted entries are reported by `--list` and `--dry-run`, but are not passed to
commands by default because the path no longer exists in the staged snapshot.

Filtering order:

1. collect staged entries from index versus `HEAD`
2. normalize paths to repository-root relative slash paths
3. apply optional pathspec filters
4. apply optional extension filters
5. drop non-executable deleted entries from command arguments
6. preserve stable index order for deterministic output

No-staged-file behavior:

- `--list` exits `0` and prints no selected executable paths
- command mode exits `0` without running the command
- `--dry-run` exits `0` and prints that the command would not run

## Output Contract

`--list` output is line oriented and stable for tests:

```text
A path/to/new.rs
M path/to/changed.ts
R old/name.js -> new/name.js
D path/to/deleted.md
```

Execution passes only executable selected paths after the user command:

```bash
zmin hooks run pre-commit --staged --ext rs,ts -- cargo fmt --check
# runs: cargo fmt --check path/to/new.rs path/to/changed.ts
```

The runner returns the child exit code. Spawn failures return Zmin validation
errors with a non-zero exit code and no stock-Git compatibility claim.

## Managed Hook Integration

Managed hooks remain optional. A generated `.git/hooks/pre-commit` wrapper may
call the staged runner, but manual hooks must remain untouched unless the user
explicitly opts into `zmin hooks add --force`.

The wrapper should be small and inspectable:

```sh
#!/bin/sh
zmin hooks run pre-commit --staged -- "$@"
```

Project-specific commands should live in Git config or an explicit checked-in
script before automatic wrapper generation is expanded. Avoid hidden defaults.

## Evidence Plan

The acceptance matrix is machine-readable at
`docs/cli/zmin_hooks_staged_runner_acceptance.tsv`.

Implementation must land in narrow slices:

1. selector library and tests for added, modified, renamed, deleted and
   unstaged-only files
2. `zmin hooks run <hook> --staged --list`
3. extension and pathspec filters
4. command execution and child exit-code propagation
5. managed-hook wrapper integration

Each slice should update the TSV status and evidence columns. The extension
inventory should count only implemented and tested surfaces as stable.
