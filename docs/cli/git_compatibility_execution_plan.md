# Git Compatibility Execution Plan

This is the step-by-step operating plan for taking Zmin from command dispatch
coverage to real Git `2.47.1` compatibility.

Use this file as the entry point when resuming work. The detailed counting
model lives in `docs/cli/git_compatibility_inventory.md`; the live slice queue,
guard mappings and latest completed slice live in
`docs/cli/variant_compatibility_plan.md`.

## Durable Plan Files

Use these files instead of chat history:

| File | Role |
| --- | --- |
| `docs/cli/git_compatibility_execution_plan.md` | step-by-step operating plan and definition of done |
| `docs/cli/git_compatibility_inventory.md` | counting model, current generated counts and command-level matrix status |
| `docs/cli/variant_compatibility_plan.md` | live next-slice pointer, immediate queue, guard classifications and closed evidence blocks |
| `docs/cli/existing_oracle_test_inventory.tsv` | generated backlog of stock-oracle test functions and whether each has TSV evidence |
| `docs/cli/matrix_row_growth_audit.md` | audited explanation of row-count growth and the required predeclared row-growth budget for future imports |
| `docs/cli/matrices/*_v2_47.tsv` | per-command behavior rows with command, option, value, combinations, state, transport, expected behavior and evidence |
| `docs/cli/zmin_extensions_inventory.md` | Zmin-only extensions kept outside the Git `2.47.1` denominator |
| `/Users/dschewchenko/work/private/.knowledge/projects/skron-core.md` | cross-session project memory that points back to the active execution plan |

## Objective

Close Git compatibility by verified behavior rows, not by command presence.

A row is useful only when it records:

- command
- option spelling
- option value or value class
- meaningful option combination
- repository state
- transport or local workflow
- platform when behavior can differ
- invalid input behavior when stock Git rejects the input
- expected stdout, stderr, exit code and observable `.git` side effects
- stock Git oracle evidence

Public documentation must keep complete command matrices at `0/151` and
complete doc-option matrices at `0/4632` until the full matrix is expanded and
closed with stock-Git evidence.

## Non-Negotiable Rules

1. Pick one row-sized slice at a time.
2. Add or update the matrix row before treating code behavior as supported.
3. Use stock Git as the oracle for stdout, stderr, exit code and side effects.
4. Close a row only with focused evidence: compat test, upstream Git test slice
   or recorded dogfood trace.
5. Do not publish support percentages from command dispatch, parser acceptance,
   represented option pairs or written-row pass rates.
6. Keep Zmin-only features in `docs/cli/zmin_extensions_inventory.md`, outside
   the Git `2.47.1` compatibility denominator.
7. Commit and push each completed slice before starting a different command,
   option class or extension.

## Resume Procedure

Run this at the start of every session:

1. Check the active Codex goal and this file.
2. Read the `Current Next Slice Pointer` in
   `docs/cli/variant_compatibility_plan.md`.
3. Run `/usr/bin/git status --short --branch` and identify unrelated staged or
   unstaged work.
4. If a WebStorm, replacement-binary or real-tool trace is blocking dogfood,
   promote it to the next slice and add a matrix row first.
5. Otherwise use `docs/cli/existing_oracle_test_inventory.tsv` to pick an
   already-covered missing-or-unclassified oracle function, or take the first
   unfinished item from the Immediate Slice Queue when no dense oracle batch is
   available.
6. Before adding behavior rows, record the source bucket and expected row delta
   from `docs/cli/matrix_row_growth_audit.md`; stop if the actual post-slice
   delta differs.
7. Confirm the exact stock Git command line and expected behavior before
   editing implementation code.

## Baseline Verification Contract

Before importing more rows from existing tests, verify the known oracle backlog
instead of discovering it implicitly during the slice:

```bash
python3 tools/git-existing-oracle-inventory.py > /tmp/zmin-oracle-inventory.tsv
cmp -s /tmp/zmin-oracle-inventory.tsv docs/cli/existing_oracle_test_inventory.tsv
awk -F '\t' 'NR==1{for(i=1;i<=NF;i++) h[$i]=i; next} { total++; c[$h["inventory_status"]]++ } END { print total, c["represented"], c["missing_or_unclassified"] }' docs/cli/existing_oracle_test_inventory.tsv
tools/git-matrix-row-delta-audit.sh 9275ac4d HEAD
```

The current frozen focused-oracle backlog is `961` functions: `678`
represented or classified and `283` `missing_or_unclassified`. Treat
`docs/cli/existing_oracle_test_inventory.tsv` as the complete current list to
walk. A docs-only row import from that list must reduce
`missing_or_unclassified` by the declared evidence-function count. If behavior
rows grow without that reduction, stop and fix the inventory or name a separate
source bucket before committing.

Use `docs/cli/matrix_row_growth_audit.md` as the ordered worklist for this
backlog, not a partial `rg` result. The current default order is the largest
coherent missing-or-unclassified buckets: `git_transport_local_compat.rs`,
`git_transport_http_compat.rs`, `git_maintenance_compat.rs`,
`git_pack_integrity_compat.rs` and `git_worktree_state_compat.rs`. Before any
TSV edit, write the selected functions and expected row/status delta in the
row-growth audit; after the edit, prove the generated inventory moved by that
declared amount.

## Slice Loop

Every compatibility slice follows this exact order:

1. Select one behavior shape.
2. Add or update the TSV matrix row.
3. Probe stock Git for the same command, option values, repository state,
   transport and platform.
4. Add focused oracle evidence.
5. Implement the smallest behavior change required for that row.
6. Run the focused test first.
7. Run `cargo check -p zmin-cli --bin zmin --profile compat`.
8. Run `tools/git-cli-readiness-status.sh`.
9. Run `tools/git-compat-command-summary.sh --tsv`.
10. Run `tools/git-compat-audit-summary.sh --tsv`.
11. Update README, inventory, variant plan and project notes with generated
    counts when counts changed.
12. Commit with a Conventional Commit message and push the branch.

Transport rows also run the relevant HTTP, SSH and git-daemon focused tests.
Replacement-binary rows also run `tools/git-replacement-dogfood-smoke.sh`.
Docs-only planning updates do not need Rust tests, but they still must preserve
the current matrix-counting rules.

## Current Work Lanes

| Order | Lane | Purpose | Stop condition |
| ---: | --- | --- | --- |
| 1 | Dogfood blockers | Close WebStorm and replacement-binary command lines that prevent using Zmin as `git` locally | replacement smoke and focused rows pass |
| 2 | Unsupported guard classification | Map every Rust `unsupported` / `not supported` hit to a Git-supported gap, invalid-input row, intentional deferral or Zmin-only extension | no raw guard is ambiguous |
| 3 | Command matrix expansion | Expand high-use commands from Git docs into options, values, negations, repeated forms, option order and repository states | command state reaches `expanding` with no false support percentage |
| 4 | Transport and platform evidence | Add HTTP, SSH, git-daemon, file/local and platform rows where behavior differs | rows have stock evidence for each relevant mode |
| 5 | Zmin-only extensions | Design extensions such as staged-file hook runner without mixing them into Git coverage | extension rows are tracked separately |

The active next slice is not duplicated here. It is the `Current Next Slice
Pointer` in `docs/cli/variant_compatibility_plan.md`, so the handoff has one
mutable pointer instead of several competing queues.

## Definition Of Done

A slice is done only when all applicable items are true:

- matrix row exists and has the right status
- stock Git oracle behavior is recorded or exercised
- focused Zmin-vs-stock evidence passes
- implementation change is scoped to the selected behavior
- generated counts are refreshed when the row set changed
- README and inventory docs still avoid false `100%` compatibility claims
- project notes mention the latest closed slice
- commit is pushed

If any item is missing, the slice stays open even if the code appears to work.

## Milestones

### M0: Honest Reporting Baseline

Keep public and internal reporting aligned on the strict model:

- complete command matrices: `0/151`
- complete doc-option matrices: `0/4632`
- written behavior rows: generated count only
- matching stock Git: generated count only
- open and invalid-input rows reported separately

### M1: Local Replacement Dogfood

Zmin can be placed first in `PATH` as `git` for common local IDE and CLI flows:

- `git --version` / `git version` compatible facade
- `status`, `log`, `diff`, `ls-files`, `rev-parse`, `config` IDE traces
- NUL output where tools use `-z`
- pathspec and date/format values observed from tools

### M2: Transport Reliability

Network and local fetch/push/clone rows cover the important transport axes:

- local path and `file://`
- smart HTTP
- SSH
- git daemon
- shallow source and shallow client states
- multiple explicit refspecs
- branchless configured fetch
- partial clone filters

### M3: Guard Burn-Down

Every Rust source guard containing `unsupported`, `not supported` or similar is
classified as one of:

- Git-supported open gap
- stock-compatible invalid input
- intentional deferral with reason
- Zmin-only validation or extension

### M4: Full Matrix Expansion

For each Git `2.47.1` command, expand documented options into values,
negations, repeated forms, order-sensitive combinations, positional modes,
repository states, transports, platforms, upstream tests and real tool traces.

### M5: Extension Backlog

After dogfood and core Git blockers, design and implement Zmin-only staged-file
hooks under the extension inventory:

- staged index files, not whole working tree
- pathspec and extension filters
- renamed and deleted file handling
- dry-run/list mode
- standard Git hooks remain compatible
