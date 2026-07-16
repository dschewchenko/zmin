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

Regenerated from the current dirty workspace on 2026-07-16 after the
replacement-readiness audit found that the checked-in inventory no longer
covered the expanded differential suite.

| Layer | Count |
| --- | ---: |
| Stock-oracle test functions found | `1466` |
| Test functions referenced by at least one matrix row, extension inventory entry or deferral entry | `1248` |
| Test functions missing or not yet classified by matrix/extension/deferral evidence | `218` |

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
| `git_global_cli_compat.rs` | `30` |
| `git_mail_series_compat.rs` | `22` |
| `git_lfs_local_compat.rs` | `18` |
| `git_history_query_compat.rs` | `16` |
| `git_credential_compat.rs` | `13` |
| `git_object_plumbing_compat.rs` | `11` |
| `git_help_compat.rs` | `11` |
| `git_admin_tools_compat.rs` | `10` |
| `git_diff_compat.rs` | `9` |
| `git_transport_local_compat.rs` | `8` |
| `git_status_compat.rs` | `7` |
| `git_ls_files_compat.rs` | `6` |
| `git_worktree_state_compat.rs` | `5` |
| `git_observed_client_compat.rs` | `10` |
| `git_index_mutation_compat.rs` | `5` |
| `git_clone_compat.rs` | `5` |
| `git_refs_compat.rs` | `4` |
| `git_rebase_interactive_compat.rs` | `4` |
| `git_replacement_dogfood_compat.rs` | `3` |

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
