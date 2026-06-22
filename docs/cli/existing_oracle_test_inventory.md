# Existing Oracle Test Inventory

This inventory prevents compatibility rows from being imported opportunistically.
It lists focused tests that already compare Zmin behavior with stock Git, then
shows whether the test function is already referenced by a behavior matrix row,
by the Zmin-only extension inventory, or by an explicit oracle deferral.

Generated TSV:

`docs/cli/existing_oracle_test_inventory.tsv`

Generator:

```bash
tools/git-existing-oracle-inventory.py --root . > docs/cli/existing_oracle_test_inventory.tsv
```

## Current Snapshot

Generated after reviewing the `git_stash_compat.rs` stash show copy-detection
backlog on `compat/status-pathspec-matrix`.

| Layer | Count |
| --- | ---: |
| Stock-oracle test functions found | `961` |
| Test functions referenced by at least one matrix row, extension inventory entry or deferral entry | `438` |
| Test functions missing or not yet classified by matrix/extension/deferral evidence | `523` |

`missing_or_unclassified` does not automatically mean "add a Git matrix row".
Each function still needs review:

- Git-compatible behavior: split into exact command/option/value/state rows.
- Git invalid input: add invalid-input rows only when stock Git rejects the
  same surface and side effects match.
- Zmin-only behavior: record under `docs/cli/zmin_extensions_inventory.md`.
- Version-mismatched, legacy or unavailable external tool behavior: record in
  `docs/cli/oracle_test_deferrals.md` until a real Git `2.47.1` oracle row can
  be added.
- Broad smoke or acceptance tests: keep as gates, not behavior rows, unless an
  exact command shape is extracted.

## Priority Buckets

Use these buckets before adding more row batches. They are sorted by number of
currently unclassified stock-oracle test functions, not by product priority.

| Test file | Missing or unclassified |
| --- | ---: |
| `git_transport_http_compat.rs` | `75` |
| `git_pack_integrity_compat.rs` | `61` |
| `git_transport_local_compat.rs` | `58` |
| `git_index_mutation_compat.rs` | `39` |
| `git_stash_compat.rs` | `38` |
| `git_maintenance_compat.rs` | `32` |
| `git_commit_compat.rs` | `26` |
| `git_worktree_state_compat.rs` | `26` |
| `git_notes_compat.rs` | `23` |
| `git_submodule_compat.rs` | `16` |
| `git_worktree_compat.rs` | `15` |
| `git_merge_compat.rs` | `13` |
| `git_sequencer_compat.rs` | `12` |
| `git_admin_tools_compat.rs` | `10` |
| `git_reflog_compat.rs` | `10` |
| `git_merge_plumbing_compat.rs` | `9` |
| `git_foreign_scm_compat.rs` | `8` |
| `git_global_cli_compat.rs` | `7` |
| `git_refs_compat.rs` | `7` |
| `git_ref_resolution_compat.rs` | `6` |
| `git_scalar_compat.rs` | `6` |
| `git_fast_import_export_compat.rs` | `5` |
| `git_cms_porcelain_compat.rs` | `4` |
| `git_object_plumbing_compat.rs` | `4` |

The full TSV is the backlog. Do not treat the table above as complete by
itself; it only summarizes the largest buckets.

## Count-Growth Audit

Behavior-row counts are allowed to grow when existing stock-oracle tests are
split into exact Git `2.47.1` matrix rows. That growth must be explicit before
the next batch starts.

Use this audit before and after row-import batches:

```bash
tools/git-matrix-row-delta-audit.sh 9275ac4d HEAD
```

The output lists commits that changed `docs/cli/matrices/*_v2_47.tsv`, sorted
in commit order. Large deltas are not compatibility regressions by themselves;
they mean the written denominator expanded. Each delta must be backed by one of
these sources:

- a focused stock-Git oracle test function in
  `existing_oracle_test_inventory.tsv`;
- an explicit Git `2.47.1` documented option/value/state row;
- a stock-compatible invalid-input row;
- a deferral or Zmin-only extension note outside the Git matrix.

Before adding more rows from `missing_or_unclassified`, record the selected
test file/function bucket and expected row count in the slice notes. After the
batch, rerun the inventory and delta audit so the count increase is traceable
to that bucket.

## Next-Row Rule

Before importing any more already-tested rows:

1. Filter `existing_oracle_test_inventory.tsv` to one test file and
   `missing_or_unclassified`.
2. Record the selected test file/function bucket and expected row count before
   editing the matrix.
3. Read the selected test function body and identify the exact command lines.
4. Check the command matrix for existing rows by command, option, value,
   combination and repo state.
5. Add a batch only when the rows share the same focused oracle function and
   do not need Rust behavior changes.
6. Regenerate this inventory and run `git-matrix-row-delta-audit.sh` after the
   batch so the backlog and denominator growth stay current.
