# Matrix Row Growth Audit

This file explains behavior-row count growth on
`compat/status-pathspec-matrix` and defines the guardrail for future matrix
imports.

## Census-First Correction

As of 2026-06-22, do not continue importing rows directly from
`docs/cli/existing_oracle_test_inventory.tsv`. The next compatibility work
starts from `docs/cli/git_compatibility_census.md` and
`docs/cli/census/remaining_to_fix_or_verify.tsv`.

`docs/cli/existing_oracle_test_inventory.tsv` is now an evidence layer only.
After a row or coherent expansion group is selected from the census, this audit
still records the source bucket, expected row/status delta and oracle evidence
movement before any TSV matrix edit.

## Why The Count Grew

The count grew because the branch imported already-existing stock-oracle test
coverage into `docs/cli/matrices/*_v2_47.tsv`.

That is legitimate compatibility accounting only when each imported row records
the full row shape:

`command + option + value + option combination + repo state + transport/local mode + platform + stdout/stderr/exit/.git side effects + stock Git evidence`

The process gap was that the known command and option seeds existed, but the
behavior-row backlog was not frozen up front. As rows were discovered in focused
compat tests, the written-row denominator moved. That made progress honest at
the row level, but operationally too surprising.

The correction is now part of this audit: the current focused-oracle backlog is
the generated and committed `docs/cli/existing_oracle_test_inventory.tsv`, and a
batch may only import rows from entries already present there unless a new
frozen inventory layer is added first. New denominator sources must be declared
as their own inventory, with counts, before any matrix rows are added from that
source.

Do not treat `151/151` commands or `4632/4632` documented option pairs as a
complete behavior denominator. They are command/option seed layers only. The
compatibility denominator grows only when a specific row shape has oracle
evidence and is written as a matrix row.

## Current Baseline

Pushed branch state audited from `9275ac4d` to `HEAD`:

| Metric | At `9275ac4d` | At `HEAD` | Delta |
| --- | ---: | ---: | ---: |
| Written behavior rows | `1094` | `2804` | `+1710` |
| Matching stock Git rows | `823` | `2415` | `+1592` |
| Open rows | `1` | `1` | `0` |
| Invalid-input rows | `270` | `388` | `+118` |
| Commands with rows | `50/151` | `105/151` | `+55` |
| Represented doc-option pairs | `253/4632` | `701/4632` | `+448` |

The text-level row delta audit must be regenerated with
`tools/git-matrix-row-delta-audit.sh 9275ac4d HEAD` after each slice. The strict
behavior row count is authoritative for row-level progress because some commits
rewrite or split existing rows rather than adding net-new row coverage.

The stock-oracle test inventory currently has `961` focused oracle functions:
`705` represented by matrix, extension or deferral evidence, and `256` still
missing or unclassified.

## Net Growth By Command

This table compares actual behavior rows per command at `9275ac4d` and at
`HEAD`.

| Command | Rows at `9275ac4d` | Rows at `HEAD` | Delta |
| --- | ---: | ---: | ---: |
| `stash` | `25` | `207` | `+182` |
| `diff` | `68` | `239` | `+171` |
| `ls-files` | `72` | `155` | `+83` |
| `diff-tree` | `0` | `74` | `+74` |
| `blame` | `101` | `171` | `+70` |
| `config` | `60` | `130` | `+70` |
| `clone` | `6` | `72` | `+66` |
| `commit` | `0` | `78` | `+78` |
| `status` | `76` | `135` | `+59` |
| `branch` | `0` | `49` | `+49` |
| `notes` | `0` | `42` | `+42` |
| `add` | `3` | `51` | `+48` |
| `fsck` | `0` | `35` | `+35` |
| `for-each-ref` | `0` | `34` | `+34` |
| `show` | `0` | `34` | `+34` |
| `remote` | `0` | `32` | `+32` |
| `cat-file` | `8` | `40` | `+32` |
| `ls-remote` | `2` | `31` | `+29` |
| `rev-list` | `0` | `28` | `+28` |
| `tag` | `0` | `27` | `+27` |
| `fetch` | `300` | `326` | `+26` |
| `bundle` | `0` | `21` | `+21` |
| `rev-parse` | `52` | `73` | `+21` |
| `apply` | `0` | `20` | `+20` |
| `check-ignore` | `0` | `20` | `+20` |
| `diff-files` | `0` | `20` | `+20` |
| `maintenance` | `0` | `20` | `+20` |
| `clean` | `12` | `30` | `+18` |
| `log` | `87` | `105` | `+18` |
| `pack-objects` | `0` | `18` | `+18` |
| `send-email` | `0` | `16` | `+16` |
| `interpret-trailers` | `0` | `15` | `+15` |
| `archive` | `1` | `17` | `+16` |
| `diff-index` | `0` | `14` | `+14` |
| `http-backend` | `0` | `14` | `+14` |
| `reflog` | `2` | `15` | `+13` |
| `describe` | `0` | `12` | `+12` |
| `filter-branch` | `2` | `14` | `+12` |
| `grep` | `0` | `12` | `+12` |
| `merge-base` | `0` | `13` | `+13` |
| `var` | `0` | `12` | `+12` |
| `check-ref-format` | `0` | `11` | `+11` |
| `pull` | `0` | `10` | `+10` |
| `rm` | `0` | `10` | `+10` |
| `check-attr` | `0` | `9` | `+9` |
| `check-mailmap` | `0` | `8` | `+8` |
| `cherry` | `0` | `8` | `+8` |
| `count-objects` | `0` | `8` | `+8` |
| `show-ref` | `10` | `18` | `+8` |
| `sparse-checkout` | `0` | `8` | `+8` |
| `symbolic-ref` | `0` | `10` | `+10` |
| `fast-import` | `0` | `7` | `+7` |
| `prune` | `0` | `7` | `+7` |
| `push` | `0` | `7` | `+7` |
| `submodule` | `0` | `7` | `+7` |
| `update-ref` | `0` | `7` | `+7` |
| `fetch-pack` | `0` | `6` | `+6` |
| `format-patch` | `0` | `6` | `+6` |
| `multi-pack-index` | `0` | `6` | `+6` |
| `patch-id` | `0` | `6` | `+6` |
| `replace` | `0` | `6` | `+6` |
| `shortlog` | `0` | `6` | `+6` |
| `verify-pack` | `0` | `6` | `+6` |
| `checkout` | `0` | `5` | `+5` |
| `hash-object` | `0` | `7` | `+7` |
| `stripspace` | `0` | `5` | `+5` |
| `column` | `0` | `4` | `+4` |
| `commit-tree` | `0` | `5` | `+5` |
| `credential` | `0` | `4` | `+4` |
| `credential-cache` | `0` | `6` | `+6` |
| `difftool` | `0` | `4` | `+4` |
| `fmt-merge-msg` | `0` | `4` | `+4` |
| `for-each-repo` | `0` | `4` | `+4` |
| `init` | `0` | `4` | `+4` |
| `ls-tree` | `0` | `6` | `+6` |
| `mailinfo` | `0` | `4` | `+4` |
| `mailsplit` | `0` | `4` | `+4` |
| `read-tree` | `0` | `5` | `+5` |
| `replay` | `0` | `4` | `+4` |
| `send-pack` | `0` | `4` | `+4` |
| `unpack-file` | `0` | `4` | `+4` |
| `version` | `0` | `4` | `+4` |
| `bugreport` | `0` | `9` | `+9` |
| `commit-graph` | `0` | `3` | `+3` |
| `credential-store` | `0` | `5` | `+5` |
| `http-fetch` | `1` | `4` | `+3` |
| `index-pack` | `0` | `3` | `+3` |
| `mktree` | `0` | `4` | `+4` |
| `range-diff` | `0` | `3` | `+3` |
| `request-pull` | `0` | `3` | `+3` |
| `cherry-pick` | `0` | `2` | `+2` |
| `get-tar-commit-id` | `0` | `2` | `+2` |
| `mktag` | `0` | `2` | `+2` |
| `quiltimport` | `0` | `2` | `+2` |
| `update-server-info` | `0` | `2` | `+2` |
| `write-tree` | `0` | `3` | `+3` |
| `am` | `0` | `1` | `+1` |
| `bisect` | `0` | `1` | `+1` |
| `checkout-index` | `0` | `7` | `+7` |
| `merge` | `0` | `1` | `+1` |
| `p4` | `0` | `1` | `+1` |
| `rebase` | `0` | `1` | `+1` |
| `rerere` | `0` | `1` | `+1` |
| `show-index` | `1` | `2` | `+1` |
| `worktree` | `0` | `1` | `+1` |

## Growth Control Rule

Before adding new behavior rows, record the selected bucket:

- source: Git docs expansion, focused stock-oracle test, upstream Git test,
  real tool trace, or guard classification
- file and function or command-option source
- expected row count delta
- expected status split: closed, open, invalid-input, or deferral
- whether Rust behavior changes are required

After the batch, rerun:

```bash
tools/git-matrix-row-delta-audit.sh 9275ac4d HEAD
tools/git-compat-command-summary.sh --tsv
tools/git-compat-audit-summary.sh --tsv
python3 tools/git-existing-oracle-inventory.py > docs/cli/existing_oracle_test_inventory.tsv
```

If actual growth differs from the declared bucket, stop and explain the
difference before committing.

## Current Known Queues

The known queues are:

- `docs/cli/existing_oracle_test_inventory.tsv`: focused stock-oracle test
  functions, currently `961` total with `256` missing or unclassified.
- `docs/cli/git_compatibility_inventory.md`: command and documented option
  seed accounting, currently `151` commands and `4632` documented
  command-option pairs.
- `docs/cli/matrices/*_v2_47.tsv`: current written behavior rows.

These are not a complete final Git behavior denominator. They are the frozen
known inventory layers that must be expanded deliberately.

## Full Backlog Verification

`docs/cli/existing_oracle_test_inventory.tsv` is the full current list of
already-existing focused stock-oracle test functions to walk before importing
more rows from that source. Do not reconstruct this list from chat history or
from a partial `rg` result.

Verify it before each oracle-import batch:

```bash
python3 tools/git-existing-oracle-inventory.py > /tmp/zmin-oracle-inventory.tsv
cmp -s /tmp/zmin-oracle-inventory.tsv docs/cli/existing_oracle_test_inventory.tsv
awk -F '\t' 'NR==1{for(i=1;i<=NF;i++) h[$i]=i; next} { total++; c[$h["inventory_status"]]++ } END { printf "total=%d represented=%d missing_or_unclassified=%d\n", total, c["represented"], c["missing_or_unclassified"] }' docs/cli/existing_oracle_test_inventory.tsv
```

Then select rows only from `missing_or_unclassified`, read the exact test
function, declare the expected row delta below and rerun the same inventory
check after the slice. The expected invariant for an import from this backlog
is:

- `represented` increases by the declared evidence-function count.
- `missing_or_unclassified` decreases by the same count.
- `behavior_rows_written` grows only by the declared row count.
- Any different movement is a process error to investigate before committing.

## Declared Oracle Import Batches

### Completed: Local Fetch Refspec Resolution Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_local_compat.rs`
- functions:
  - `fetch_direct_location_resolves_short_remote_name_but_not_short_tag_like_stock_git`
  - `fetch_direct_location_lhs_refspec_disambiguation_like_stock_git`
  - `fetch_explicit_head_to_branch_backfills_tags_like_stock_git`
  - `fetch_direct_file_url_accepts_multiple_explicit_refspecs_like_stock_git`
  - `fetch_empty_refmap_with_branch_disables_configured_refspec_like_stock_git`
- expected row delta: `+5` behavior rows in
  `docs/cli/matrices/fetch_v2_47.tsv`
- expected status split: `+5` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+5`,
  missing_or_unclassified `-5`
- Rust behavior changes required: no

### Completed: Smart HTTP Fetch Pack Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `fetch_reads_smart_http_pack_like_stock_git`
  - `fetch_smart_http_wildcard_refspec_updates_remote_refs_like_stock_git`
  - `fetch_smart_http_incremental_thin_pack_repairs_existing_bases_like_stock_git`
  - `fetch_smart_http_noop_skips_upload_pack_when_roots_exist_locally`
  - `fetch_smart_http_multiple_explicit_tags_with_protocol_v2_like_stock_git`
- expected row delta: `+5` behavior rows in
  `docs/cli/matrices/fetch_v2_47.tsv`
- expected status split: `+5` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+5`,
  missing_or_unclassified `-5`
- Rust behavior changes required: no

### Completed: Local Fetch-Pack Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_local_compat.rs`
- functions:
  - `fetch_pack_copies_local_ref_objects_like_stock_git`
  - `fetch_pack_accepts_thin_and_no_progress_like_stock_git`
  - `fetch_pack_include_tag_copies_annotated_tag_objects_like_stock_git`
  - `fetch_pack_include_tag_with_depth_limited_like_stock_git`
  - `fetch_pack_include_tag_depth_includes_nested_annotated_tags_like_stock_git`
  - `fetch_pack_depth_one_like_stock_git`
- expected row delta: `+6` behavior rows in
  `docs/cli/matrices/fetch_pack_v2_47.tsv`
- expected status split: `+6` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+6`,
  missing_or_unclassified `-6`
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+6` behavior rows,
  `+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+6`
  represented oracle functions, `-6` missing-or-unclassified oracle functions,
  `+1` command with rows and `+3` represented doc-option pairs.

### Completed: HTTP Backend Upload-Pack Filter Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `http_backend_upload_pack_filter_blob_none_omits_blob_objects`
  - `http_backend_upload_pack_filter_blob_limit_omits_large_blobs`
  - `http_backend_upload_pack_filter_object_type_blob_omits_trees`
  - `http_backend_upload_pack_filter_tree_depth_limits_tree_walk`
  - `http_backend_upload_pack_filter_combine_applies_all_filters`
  - `http_backend_upload_pack_filter_sparse_oid_omits_unmatched_blobs`
- expected row delta: `+6` behavior rows in
  `docs/cli/matrices/http_backend_v2_47.tsv`
- expected status split: `+6` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+6`,
  missing_or_unclassified `-6`
- expected command/doc-option movement: `+1` command with rows,
  `+0` represented doc-option pairs; `http-backend` has no documented CLI
  option spelling seed in the current Git docs inventory
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+6` behavior rows,
  `+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+6`
  represented oracle functions, `-6` missing-or-unclassified oracle functions,
  `+1` command with rows and `+0` represented doc-option pairs.

### Completed: Local Send-Pack Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_local_compat.rs`
- functions:
  - `send_pack_updates_local_bare_ref_like_stock_git`
  - `send_pack_thin_updates_local_bare_ref_like_stock_git`
  - `send_pack_mirror_syncs_heads_tags_and_deletions_like_stock_git`
  - `send_pack_atomic_rejects_all_updates_when_one_ref_fails`
- expected row delta: `+4` behavior rows in
  `docs/cli/matrices/send_pack_v2_47.tsv`
- expected status split: `+3` closed rows, `0` open rows, `+1` invalid-input
  row
- expected oracle inventory delta: represented `+4`,
  missing_or_unclassified `-4`
- expected command/doc-option movement: `+1` command with rows,
  `+3` represented doc-option pairs for `--thin`, `--mirror` and `--atomic`
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+4` behavior rows,
  `+3` closed rows, `+0` open rows, `+1` invalid-input row, `+4`
  represented oracle functions, `-4` missing-or-unclassified oracle functions,
  `+1` command with rows and `+3` represented doc-option pairs.

### Completed: HTTP Backend Upload-Pack Deepen Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `http_backend_upload_pack_deepen_emits_shallow_boundary_and_depth_limited_pack`
  - `http_backend_upload_pack_deepen_since_emits_time_limited_pack`
  - `http_backend_upload_pack_deepen_not_excludes_named_ref_history`
  - `http_backend_upload_pack_deepen_relative_extends_existing_shallow_boundary`
- expected row delta: `+4` behavior rows in
  `docs/cli/matrices/http_backend_v2_47.tsv`
- expected status split: `+4` closed rows, `0` open rows, `0` invalid-input
  rows
- expected oracle inventory delta: represented `+4`,
  missing_or_unclassified `-4`
- expected command/doc-option movement: `+0` commands with rows,
  `+0` represented doc-option pairs; `http-backend` already has matrix rows
  and no documented CLI option spelling seed in the current Git docs inventory
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+4` behavior rows,
  `+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+4`
  represented oracle functions, `-4` missing-or-unclassified oracle functions,
  `+0` commands with rows and `+0` represented doc-option pairs.

### Completed: HTTP Fetch Packfile Batch

- source: focused stock-oracle test backlog
- file: `crates/zmin-cli/tests/git_transport_http_compat.rs`
- functions:
  - `http_fetch_packfile_downloads_and_indexes_pack`
  - `http_fetch_packfile_requires_index_pack_args_like_stock_git`
- expected row delta: `+3` behavior rows in
  `docs/cli/matrices/http_fetch_v2_47.tsv`
- expected status split: `+2` closed rows, `0` open rows, `+1`
  invalid-input row
- expected oracle inventory delta: represented `+2`,
  missing_or_unclassified `-2`
- expected command/doc-option movement: `+0` commands with rows,
  `+1` represented doc-option pair for `--packfile`; `--index-pack-arg` is
  already represented by the existing invalid delegated-argument row
- Rust behavior changes required: no
- actual post-import movement matched the declaration: `+3` behavior rows,
  `+2` closed rows, `+0` open rows, `+1` invalid-input row, `+2`
  represented oracle functions, `-2` missing-or-unclassified oracle functions,
  `+0` commands with rows and `+1` represented doc-option pair.

## Frozen Oracle Backlog Snapshot

This snapshot explains the remaining known denominator growth from focused
stock-oracle tests. It is intentionally file-level, not row-level: each listed
test function still must be read before adding TSV rows, because one function
can prove one row, several command variants, or a non-Git extension/deferral.

As of this commit, `docs/cli/existing_oracle_test_inventory.tsv` contains `961`
focused oracle functions. `705` are already represented by matrix rows,
extension rows or explicit deferrals, and `256` are
`missing_or_unclassified`.

Largest missing/unclassified buckets:

| Test file | Missing/unclassified functions |
| --- | ---: |
| `git_transport_http_compat.rs` | `21` |
| `git_transport_local_compat.rs` | `22` |
| `git_worktree_state_compat.rs` | `22` |
| `git_maintenance_compat.rs` | `21` |
| `git_pack_integrity_compat.rs` | `18` |
| `git_submodule_compat.rs` | `16` |
| `git_worktree_compat.rs` | `15` |
| `git_notes_compat.rs` | `14` |
| `git_merge_compat.rs` | `13` |
| `git_sequencer_compat.rs` | `10` |
| `git_admin_tools_compat.rs` | `10` |
| `git_merge_plumbing_compat.rs` | `9` |
| `git_foreign_scm_compat.rs` | `8` |
| `git_index_mutation_compat.rs` | `7` |
| `git_refs_compat.rs` | `7` |
| `git_scalar_compat.rs` | `6` |
| `git_ref_resolution_compat.rs` | `6` |
| `git_global_cli_compat.rs` | `5` |
| `git_fast_import_export_compat.rs` | `5` |
| `git_object_plumbing_compat.rs` | `4` |
| `git_cms_porcelain_compat.rs` | `4` |
| `git_sparse_checkout_compat.rs` | `3` |
| `git_mail_tools_compat.rs` | `3` |
| `git_clone_compat.rs` | `2` |
| `compatibility_command.rs` | `1` |
| `git_stash_compat.rs` | `1` |
| `git_repository_state_compat.rs` | `1` |
| `git_mail_series_compat.rs` | `1` |
| `git_cli_failure_compat.rs` | `1` |

Largest command-hint buckets inside those `256` functions:

| Command hint | Missing/unclassified functions |
| --- | ---: |
| `<none>` | `59` |
| `worktree` | `43` |
| `merge` | `28` |
| `remote` | `24` |
| `maintenance` | `23` |
| `submodule` | `17` |
| `branch` | `16` |
| `refs` | `15` |
| `commit` | `14` |
| `notes` | `14` |
| `add` | `13` |
| `checkout` | `7` |

## Oracle Import Walk Order

Until the `missing_or_unclassified` queue is empty, use this order for
docs-only imports from existing focused tests:

1. Prefer the largest file bucket that can produce a coherent 3-10 row batch
   without Rust changes.
2. Within that file, prefer functions that share one command and one behavior
   shape, such as one config family, one transport mode or one state transition.
3. Before editing TSV rows, write the exact evidence functions and expected
   row/status delta in `Latest Declared Import`.
4. After editing, regenerate `docs/cli/existing_oracle_test_inventory.tsv` and
   require the missing count to decrease by the declared evidence-function
   count.

For the current snapshot, the default candidate order is:

| Order | Bucket | Why first |
| ---: | --- | --- |
| 1 | `git_transport_local_compat.rs` (`22`) | remaining local/file transport and remote-management rows |
| 2 | `git_worktree_state_compat.rs` (`22`) | remaining worktree state rows that may expose implementation gaps |
| 3 | `git_transport_http_compat.rs` (`21`) | remaining network transport coverage over HTTP, SSH and git-daemon |
| 4 | `git_maintenance_compat.rs` (`21`) | remaining dense maintenance/repack/multi-pack-index rows |
| 5 | `git_pack_integrity_compat.rs` (`18`) | remaining pack/fsck/bundle rows; continue here only when the selected function group is coherent |

If a new WebStorm or replacement-binary blocker appears, it overrides this
walk order. If a selected bucket produces Zmin-only extension behavior or an
intentional deferral instead of Git matrix rows, record that classification and
do not increase written behavior rows.

### Completed: Local Fetch Prune Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_local_compat.rs`, local fetch prune behavior group.

Evidence functions:

- `git_transport_local_compat::fetch_prune_with_branch_name_keeps_other_remote_tracking_refs_like_stock_git`
- `git_transport_local_compat::fetch_prune_config_prunes_stale_remote_tracking_refs_like_stock_git`
- `git_transport_local_compat::fetch_prune_only_prints_remote_url_header_like_stock_git`
- `git_transport_local_compat::fetch_prune_resolves_remote_tracking_directory_file_conflict_like_stock_git`
- `git_transport_local_compat::fetch_prune_tags_config_with_prune_config_removes_stale_tags_like_stock_git`
- `git_transport_local_compat::fetch_direct_file_url_prune_tags_prunes_tags_but_keeps_remote_tracking_refs_like_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `fetch --prune`,
  `--prune-tags` and `<none>` already had rows
- Rust behavior changes: no

Expected rows:

- `git fetch --prune origin main`
- `git -c fetch.prune=true fetch origin`
- `git fetch --prune origin` with a stale remote-tracking branch
- `git fetch --prune` resolving a remote-tracking directory/file conflict
- `git -c fetch.prune=true -c fetch.pruneTags=true fetch origin`
- `git fetch file://repo --prune --prune-tags`

The evidence checks stock-compatible local/file transport prune behavior:
branch-limited prune keeps unrelated stale remote-tracking refs, prune config
removes stale remote-tracking refs, prune stderr begins with the `From <url>`
header, prune resolves a remote-tracking D/F conflict, pruneTags config removes
stale tags with prune enabled, and direct file URL pruneTags prunes tags while
keeping remote-tracking refs.

Actual post-import movement matched the declaration: `+6` behavior rows, `+6`
closed rows, `+0` open rows, `+0` invalid-input rows, `+6` represented oracle
functions, `-6` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`, network clone shallow/shared behavior group.

Evidence functions:

- `git_transport_http_compat::clone_reads_shallow_ssh_remote_like_stock_git`
- `git_transport_http_compat::clone_shared_is_ignored_for_ssh_remote_like_stock_git`
- `git_transport_http_compat::clone_reads_shallow_smart_http_pack_like_stock_git`
- `git_transport_http_compat::clone_reads_shallow_smart_http_tags_like_stock_git`
- `git_transport_http_compat::clone_shared_is_ignored_for_smart_http_like_stock_git`
- `git_transport_http_compat::clone_reads_shallow_dumb_http_repository_like_stock_git`
- `git_transport_http_compat::clone_shared_is_ignored_for_dumb_http_like_stock_git`
- `git_transport_http_compat::clone_reads_shallow_git_daemon_remote_like_stock_git`
- `git_transport_http_compat::clone_shared_is_ignored_for_git_daemon_like_stock_git`

Expected movement:

- behavior rows: `+9`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+9`
- missing-or-unclassified oracle functions: `-9`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `clone --depth` and
  `clone --shared` already had rows
- Rust behavior changes: no

Expected rows:

- `GIT_SSH_COMMAND=<fake-ssh> git clone --depth=1 ssh://host/path/remote.git dst`
- `GIT_SSH_COMMAND=<fake-ssh> git clone --shared ssh://host/path/remote.git dst`
- `git clone --depth=1 http://host/remote.git dst` over smart HTTP
- `git clone --depth=1 http://host/remote.git dst` over smart HTTP with tags
- `git clone --shared http://host/remote.git dst` over smart HTTP
- `git clone --depth=1 http://host/.git dst` over dumb HTTP
- `git clone --shared http://host/.git dst` over dumb HTTP
- `git clone --depth=1 git://127.0.0.1:<port>/remote.git dst`
- `git clone --shared git://127.0.0.1:<port>/remote.git dst`

The evidence compares stock Git and Zmin clone results for network shallow
state, tag/ref handling and the non-local `--shared` behavior where stock Git
does not write an alternates file. The dumb HTTP shallow row is
stock-compatible invalid input because stock Git rejects shallow capabilities
for dumb HTTP.

Actual post-import movement matched the declaration: `+9` behavior rows, `+8`
closed rows, `+0` open rows, `+1` invalid-input row, `+9` represented oracle
functions, `-9` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_local_compat.rs`, remote-HEAD fetch behavior group.

Evidence functions:

- `git_transport_local_compat::fetch_follow_remote_head_never_does_not_recreate_remote_head`
- `git_transport_local_compat::fetch_default_follow_remote_head_preserves_existing_remote_head`
- `git_transport_local_compat::fetch_explicit_refspec_does_not_update_remote_head_like_stock_git`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `fetch <none>` and
  `<refspec>` already had rows
- Rust behavior changes: no

Expected rows:

- `git -c remote.origin.followRemoteHEAD=never fetch`
- `git fetch` with an existing manually set `refs/remotes/origin/HEAD`
- `git fetch origin refs/heads/main:refs/remotes/origin/main` with
  `remote.origin.followRemoteHEAD=always`

The evidence checks stock-compatible remote HEAD handling for configured local
remotes: `followRemoteHEAD=never` does not recreate a deleted remote HEAD, a
default fetch preserves an existing remote HEAD, and an explicit refspec fetch
does not update remote HEAD even when followRemoteHEAD is `always`.

Actual post-import movement matched the declaration: `+3` behavior rows, `+3`
closed rows, `+0` open rows, `+0` invalid-input rows, `+3` represented oracle
functions, `-3` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`.

Evidence functions:

- `git_transport_http_compat::clone_reads_git_daemon_remote_like_stock_git`
- `git_transport_http_compat::clone_reads_ssh_remote_like_stock_git`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `clone <repository>` already
  had rows
- Rust behavior changes: no

Expected rows:

- `git clone git://127.0.0.1:<port>/remote.git dst`
- `GIT_SSH_COMMAND=<fake-ssh> git clone ssh://host/path/remote.git dst`

The evidence asserts successful execution and compares stock Git and Zmin
checked-out file contents, `HEAD` and refs for default clone over git-daemon
and SSH transports.

Actual post-import movement matched the declaration: `+2` behavior rows, `+2`
closed rows, `+0` open rows, `+0` invalid-input rows, `+2` represented oracle
functions, `-2` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`.

Evidence functions:

- `git_transport_http_compat::fetch_reads_git_daemon_remote_like_stock_git`
- `git_transport_http_compat::fetch_reads_ssh_remote_like_stock_git`
- `git_transport_http_compat::fetch_ssh_wildcard_refspec_prune_no_tags_like_stock_git`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `<none>`, `<refspec>`,
  `--prune` and `--no-tags` already had `fetch` rows
- Rust behavior changes: no

Expected rows:

- `git fetch origin` with a configured git-daemon remote
- `GIT_SSH_COMMAND=<fake-ssh> git fetch origin` with a configured SSH remote
- `GIT_SSH_COMMAND=<fake-ssh> git fetch origin +refs/heads/*:refs/remotes/origin/* --prune --no-tags`

The evidence asserts successful execution and compares stock Git and Zmin
remote-tracking refs plus fetched object contents for the default transport
fetches and the explicit wildcard/prune/no-tags SSH fetch.

Actual post-import movement matched the declaration: `+3` behavior rows, `+3`
closed rows, `+0` open rows, `+0` invalid-input rows, `+3` represented oracle
functions, `-3` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`.

Evidence function:

- `git_transport_http_compat::ls_remote_reads_ssh_remote_like_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `<repository>`, `--heads`,
  `--tags`, `--refs` and `<pattern>` already had `ls-remote` rows
- Rust behavior changes: no

Expected rows:

- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote ssh://host/path/remote.git`
- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote --heads ssh://host/path/remote.git`
- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote --tags ssh://host/path/remote.git`
- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote --refs ssh://host/path/remote.git`
- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote ssh://host/path/remote.git v*`
- `GIT_SSH_COMMAND=<fake-ssh> git ls-remote host:path/remote.git`

The evidence asserts successful execution and compares stock Git and Zmin
stdout for SSH transport across default repository listing, head filtering, tag
filtering, `--refs` filtering, a pattern argument and scp-like URL syntax.

Actual post-import movement matched the declaration: `+6` behavior rows, `+6`
closed rows, `+0` open rows, `+0` invalid-input rows, `+1` represented oracle
function, `-1` missing-or-unclassified oracle function, `+0` commands with rows
and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`.

Evidence function:

- `git_transport_http_compat::ls_remote_reads_git_daemon_remote_like_stock_git`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `<repository>`, `--heads`,
  `--tags`, `--refs` and `<pattern>` already had `ls-remote` rows
- Rust behavior changes: no

Expected rows:

- `git ls-remote git://127.0.0.1:<port>/remote.git`
- `git ls-remote --heads git://127.0.0.1:<port>/remote.git`
- `git ls-remote --tags git://127.0.0.1:<port>/remote.git`
- `git ls-remote --refs git://127.0.0.1:<port>/remote.git`
- `git ls-remote git://127.0.0.1:<port>/remote.git v*`

The evidence compares stock Git and Zmin stdout, stderr and exit code for
git-daemon transport across default repository listing, head filtering, tag
filtering, `--refs` filtering and a pattern argument.

Actual post-import movement matched the declaration: `+5` behavior rows, `+5`
closed rows, `+0` open rows, `+0` invalid-input rows, `+1` represented oracle
function, `-1` missing-or-unclassified oracle function, `+0` commands with rows
and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, first frozen backlog bucket
`git_transport_http_compat.rs`.

Evidence functions:

- `git_transport_http_compat::ls_remote_reads_dumb_http_info_refs_like_stock_git`
- `git_transport_http_compat::ls_remote_reads_smart_http_info_refs_like_stock_git`

Expected movement:

- behavior rows: `+10`
- closed rows: `+10`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: expected `+3` for `ls-remote --heads`,
  `ls-remote --tags` and `ls-remote <pattern>`; `<repository>` and `--refs`
  already had rows
- Rust behavior changes: no

Expected rows:

- `git ls-remote http://127.0.0.1:<port>/.git`
- `git ls-remote --heads http://127.0.0.1:<port>/.git`
- `git ls-remote --tags http://127.0.0.1:<port>/.git`
- `git ls-remote --refs http://127.0.0.1:<port>/.git`
- `git ls-remote http://127.0.0.1:<port>/.git v*`
- `git ls-remote http://127.0.0.1:<port>/remote.git`
- `git ls-remote --heads http://127.0.0.1:<port>/remote.git`
- `git ls-remote --tags http://127.0.0.1:<port>/remote.git`
- `git ls-remote --refs http://127.0.0.1:<port>/remote.git`
- `git ls-remote http://127.0.0.1:<port>/remote.git v*`

The evidence compares stock Git and Zmin stdout, stderr and exit code for
dumb HTTP `info/refs` and smart HTTP discovery across default repository
listing, head filtering, tag filtering, `--refs` filtering and a pattern
argument.

Actual post-import movement matched the corrected declaration: `+10` behavior
rows, `+10` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions,
`+0` commands with rows and `+3` represented doc-option pairs. The pre-edit
estimate expected `+2` represented doc-option pairs, but generated summary
correctly counted `ls-remote <pattern>` as a represented documented seed too;
this correction was made before committing.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_pack_integrity_compat::fsck_gitmodules_blob_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_missing_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_name_config_validation_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_url_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_path_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_update_severity_config_matches_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+6`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.gitmodulesBlob=bogus fsck`
- `git -c fsck.gitmodulesMissing=bogus fsck`
- `git -c fsck.gitmodulesName=bogus fsck`
- `git -c fsck.gitmodulesUrl=bogus fsck`
- `git -c fsck.gitmodulesPath=bogus fsck`
- `git -c fsck.gitmodulesUpdate=bogus fsck`

The evidence compares stock Git and Zmin output and exit status for invalid
`fsck.<message>` severity config values against invalid `.gitmodules` object
and content states.

Actual post-import movement matched the declaration: `+6` behavior rows, `+0`
closed rows, `+0` open rows, `+6` invalid-input rows, `+6` represented oracle
functions, `-6` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_pack_integrity_compat::fsck_tree_not_sorted_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_special_tree_name_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_full_pathname_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_duplicate_entries_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_null_sha1_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_gitmodules_parse_severity_config_matches_stock_git`

Expected movement:

- behavior rows: `+8`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+8`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.treeNotSorted=bogus fsck`
- `git -c fsck.hasDot=bogus fsck`
- `git -c fsck.hasDotdot=bogus fsck`
- `git -c fsck.hasDotgit=bogus fsck`
- `git -c fsck.fullPathname=bogus fsck`
- `git -c fsck.duplicateEntries=bogus fsck`
- `git -c fsck.nullSha1=bogus fsck`
- `git -c fsck.gitmodulesParse=bogus fsck`

The evidence compares stock Git and Zmin output and exit status for invalid
`fsck.<message>` severity config values against malformed tree objects and
malformed `.gitmodules` content.

Actual post-import movement matched the declaration: `+8` behavior rows, `+0`
closed rows, `+0` open rows, `+8` invalid-input rows, `+6` represented oracle
functions, `-6` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_pack_integrity_compat::fsck_missing_space_before_email_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_name_before_email_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_space_before_date_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_zero_padded_date_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_zero_padded_filemode_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_filemode_severity_config_matches_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+6`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.missingSpaceBeforeEmail=bogus fsck`
- `git -c fsck.missingNameBeforeEmail=bogus fsck`
- `git -c fsck.missingSpaceBeforeDate=bogus fsck`
- `git -c fsck.zeroPaddedDate=bogus fsck`
- `git -c fsck.zeroPaddedFilemode=bogus fsck`
- `git -c fsck.badFilemode=bogus fsck`

The evidence compares stock Git and Zmin output and exit status for invalid
`fsck.<message>` severity config values against malformed commit and tree
objects.

Actual post-import movement matched the declaration: `+6` behavior rows, `+0`
closed rows, `+0` open rows, `+6` invalid-input rows, `+6` represented oracle
functions, `-6` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Classification

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, classified as Zmin-only
extension evidence rather than Git `2.47.1` matrix coverage.

Evidence functions:

- `git_transport_http_compat::clone_instant_git_daemon_materializes_head_then_fetch_hydrates_refs`
- `git_transport_http_compat::clone_instant_git_daemon_demand_hydrate_recovers_missing_head_objects`
- `git_transport_http_compat::clone_instant_git_daemon_background_fetch_hydrates_refs`
- `git_transport_http_compat::clone_instant_ssh_materializes_head_then_fetch_hydrates_refs`
- `git_transport_http_compat::clone_instant_ssh_demand_hydrate_recovers_missing_head_objects`
- `git_transport_http_compat::clone_instant_ssh_background_fetch_hydrates_refs`
- `git_transport_http_compat::clone_instant_smart_http_materializes_head_then_fetch_hydrates_refs`
- `git_transport_http_compat::clone_instant_smart_http_demand_hydrate_recovers_missing_head_objects`
- `git_transport_http_compat::clone_instant_smart_http_background_fetch_hydrates_refs`

Expected movement:

- behavior rows: `+0`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+9`
- missing-or-unclassified oracle functions: `-9`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- Rust behavior changes: no

Classification:

- `zmin clone --instant` over git-daemon, SSH and smart HTTP is an additive
  Zmin clone mode and not a Git `2.47.1` option row.
- `zmin clone --instant --background-fetch` over git-daemon, SSH and smart HTTP
  is a Zmin-only extension mode.
- `zmin clone --instant --demand-hydrate` over git-daemon, SSH and smart HTTP
  is a Zmin-only extension mode.

Actual post-classification movement matched the declaration: `+0` behavior
rows, `+0` closed rows, `+0` open rows, `+0` invalid-input rows, `+9`
represented oracle functions, `-9` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

Use this snapshot as the upper bound for already-known oracle-import growth.
Future slices should reduce the `missing_or_unclassified` count by their
declared evidence-function count. If a slice increases written TSV rows without
reducing this count or without naming a different source bucket, that is a
process error.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_hook_failures_match_stock_git_flow`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -m fail` with failing `pre-commit`
- `git commit -m fail` with failing `commit-msg`
- `git commit -m post` with failing `post-commit`

The evidence compares stock Git and Zmin output, exit status and repository
state for blocked and post-commit hook flows.

Actual post-import movement matched the declaration: `+3` behavior rows,
`+3` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+0` represented doc-option pairs.

Source bucket: census-selected implemented-but-unverified `cherry-pick`
schema rows, using focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv` as the evidence layer.

Evidence functions:

- `git_sequencer_compat::cherry_pick_and_revert_match_stock_git_for_clean_single_commit`
- `git_sequencer_compat::cherry_pick_and_revert_mainline_merge_match_stock_git`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+1`
- represented doc-option pairs: expected `+1` for the shared `mainline`
  parser surface
- Rust behavior changes: no

Expected rows:

- `git cherry-pick <feature-commit>`
- `git cherry-pick -m 1 <merge-commit>`

The evidence compares stock Git and Zmin exit status, stdout/stderr, resulting
tree, commit subject and clean worktree state for clean single-commit input
and selected-mainline merge input.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions,
`+1` command with rows and `+1` represented doc-option pair.

## Latest Declared Import

Source bucket: census-selected implemented-but-unverified `verify-pack`
`<positional:packs>` schema row, using already represented focused
stock-oracle evidence from `docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_pack_integrity_compat::verify_pack_matches_stock_git_for_default_and_stats`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; this closes the Zmin schema
  positional `packs` parser surface for the already represented default
  verify-pack case
- Rust behavior changes: no

Expected row:

- `git verify-pack .git/objects/pack/pack-*.idx`

The evidence compares stock Git and Zmin default verify-pack stdout for the
same local pack index.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: census-selected implemented-but-unverified positional schema
rows for object plumbing commands, using already represented focused
stock-oracle evidence from `docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_object_plumbing_compat::hash_object_and_cat_file_match_stock_git`
- `git_object_plumbing_compat::unpack_file_matches_stock_git_blob_behavior`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; these close the Zmin schema
  positional `paths` and `object` parser surfaces for already represented
  stock-Git parity cases
- Rust behavior changes: no

Expected rows:

- `git hash-object a.txt`
- `git unpack-file <blob>`

The evidence compares stock Git and Zmin object ids for path hashing, and the
temporary worktree file content for unpacking a blob object.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_pack_integrity_compat::verify_pack_matches_stock_git_for_default_and_stats`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`; `verify-pack` already has matrix rows
- represented doc-option pairs: expected `+2` for `verify-pack -v` and
  `verify-pack -s` if those documented short options were not already counted
- Rust behavior changes: no

Expected rows:

- `git verify-pack <idx>`
- `git verify-pack -v <idx>`
- `git verify-pack -s <idx>`

The evidence compares stock Git and Zmin stdout for default verification,
verbose object listing and statistics output against the same local pack index.

Actual post-import movement matched the declaration: `+3` behavior rows, `+3`
closed rows, `+0` open rows, `+0` invalid-input rows, `+1` represented oracle
function, `-1` missing-or-unclassified oracle function, `+0` commands with
rows and `+2` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_pack_integrity_compat::bundle_create_list_heads_and_unbundle_are_stock_readable`
- `git_pack_integrity_compat::bundle_create_accepts_version_option_for_upstream_fetch_suite`
- `git_pack_integrity_compat::bundle_create_accepts_since_option_for_upstream_fetch_suite`
- `git_pack_integrity_compat::bundle_unbundle_accepts_prerequisite_bundles`

Expected movement:

- behavior rows: `+10`
- closed rows: `+10`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`; `bundle` already has matrix rows
- represented doc-option pairs: expected `+1`; `bundle --version` and bundle
  path rows already have represented rows, while `bundle --since` becomes newly
  represented if it was not already counted
- Rust behavior changes: no

Expected rows:

- `git bundle create repo.bundle HEAD <branch>`
- `git bundle list-heads repo.bundle`
- `git bundle list-heads repo.bundle refs/heads/main`
- `git bundle list-heads repo.bundle refs/heads/*`
- `git bundle unbundle repo.bundle`
- `git bundle unbundle repo.bundle refs/heads/*`
- `git bundle create --version=3 versioned.bundle main^..main`
- `git bundle create since.bundle main --since=<date>`
- `git bundle verify incremental.bundle` with prerequisites satisfied
- `git bundle unbundle incremental.bundle` with prerequisites satisfied

The evidence checks stock-readable Zmin-created bundle payloads, stock-matching
`list-heads` and `unbundle` stdout, fetchability of a versioned bundle,
`--since` bundle verification, and prerequisite bundle verify/unbundle behavior.

Actual post-import movement matched the declaration: `+10` behavior rows,
`+10` closed rows, `+0` open rows, `+0` invalid-input rows, `+4` represented
oracle functions, `-4` missing-or-unclassified oracle functions, `+0` commands
with rows and `+1` represented doc-option pair.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_maintenance_compat::multi_pack_index_repack_batch_size_one_noops_like_stock_git`
- `git_maintenance_compat::multi_pack_index_repack_and_expire_consolidate_like_stock_git`
- `git_maintenance_compat::maintenance_incremental_repack_writes_stock_verifiable_multi_pack_index`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`; `multi-pack-index` and `maintenance` already
  have matrix rows
- represented doc-option pairs: expected `+1`; `maintenance --task` already
  has represented rows, and `multi-pack-index --batch-size` becomes newly
  represented if it was not already counted
- Rust behavior changes: no

Expected rows:

- `git multi-pack-index repack --batch-size=1`
- `git multi-pack-index repack --batch-size=0` followed by
  `git multi-pack-index expire`
- `git maintenance run --task=incremental-repack`

The evidence compares stock Git and Zmin pack-file effects, no-op behavior for
small repack batches, consolidation plus expire behavior, stock-verifiable
multi-pack-index metadata and `fsck --strict`/`multi-pack-index verify`
side effects.

Actual post-import movement matched the declaration: `+3` behavior rows, `+3`
closed rows, `+0` open rows, `+0` invalid-input rows, `+3` represented oracle
functions, `-3` missing-or-unclassified oracle functions, `+0` commands with
rows and `+1` represented doc-option pair.

### Declared: Push Network Transport Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, current largest missing bucket
`git_transport_http_compat.rs`, push network transport behavior group.

Evidence functions:

- `git_transport_http_compat::push_writes_ssh_remote_like_stock_git`
- `git_transport_http_compat::push_writes_smart_http_remote_like_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: to be confirmed by generated summary because
  this extends existing `push` coverage and may add the first `push -u` row
- Rust behavior changes: no

Expected rows:

- `GIT_SSH_COMMAND=<fake-ssh> git push -u origin main`
- `GIT_SSH_COMMAND=<fake-ssh> git push origin feature`
- `GIT_SSH_COMMAND=<fake-ssh> git push origin :feature`
- `git push -u origin main` over smart HTTP
- `git push origin feature` over smart HTTP
- `git push origin :feature` over smart HTTP
The evidence compares stock Git and Zmin remote refs, pushed object contents,
feature branch creation/deletion and upstream branch config after pushes over
SSH and smart HTTP transports.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions,
`+0` commands with rows and `+1` represented doc-option pair.

### Declared: Pack Objects Readable Pack Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, current largest missing bucket
`git_pack_integrity_compat.rs`, pack-objects readable-pack behavior group.

Evidence functions:

- `git_pack_integrity_compat::pack_objects_stdout_writes_pack_readable_by_stock_git`
- `git_pack_integrity_compat::pack_objects_progress_flags_write_stock_readable_pack`
- `git_pack_integrity_compat::pack_objects_undeltified_compat_flags_write_stock_readable_pack`
- `git_pack_integrity_compat::pack_objects_window_depth_writes_stock_readable_delta_pack`
- `git_pack_integrity_compat::pack_objects_base_name_writes_stock_readable_pack_and_index`

Expected movement:

- behavior rows: `+8`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+5`
- missing-or-unclassified oracle functions: `-5`
- commands with rows: `+0`
- represented doc-option pairs: to be confirmed by generated summary because
  this extends existing `pack-objects` coverage and may add the first rows for
  `--stdout`, `--revs`, progress and delta-control options
- Rust behavior changes: no

Expected rows:

- `git pack-objects --stdout --revs`
- `git pack-objects --stdout --revs --progress`
- `git pack-objects --stdout --revs --no-progress`
- `git pack-objects --stdout --revs --no-reuse-delta`
- `git pack-objects --stdout --revs --no-reuse-object`
- `git pack-objects --stdout --revs --delta-base-offset`
- `git pack-objects --window=10 --depth=10 .git/objects/pack/pack-zmin`
- `git pack-objects .git/objects/pack/pack-zmin`

The evidence verifies Zmin-created packs with stock Git by unpacking objects,
checking object contents, verifying delta linkage with `verify-pack -v`, and
checking that basename mode writes stock-readable `.pack` and `.idx` files.

Actual post-import movement matched the declaration: `+8` behavior rows,
`+8` closed rows, `+0` open rows, `+0` invalid-input rows, `+5`
represented oracle functions, `-5` missing-or-unclassified oracle functions,
`+0` commands with rows and `+6` represented doc-option pairs.

### Declared: Maintenance Prefetch Local Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, current largest missing bucket
`git_maintenance_compat.rs`, maintenance prefetch local/no-remote behavior
group.

Evidence functions:

- `git_maintenance_compat::maintenance_prefetch_noops_without_remotes_like_stock_git`
- `git_maintenance_compat::maintenance_prefetch_local_remote_writes_prefetch_refs_like_stock_git`
- `git_maintenance_compat::maintenance_prefetch_unsupported_remote_helper_failure_matches_stock_git`

Expected movement:

- behavior rows: `+4`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: to be confirmed by generated summary because
  this extends existing `maintenance --task` coverage and may add the first
  `maintenance --quiet` row
- Rust behavior changes: no

Expected rows:

- `git maintenance run --task=prefetch` in a repository without remotes
- `git maintenance run --task=gc --task=prefetch --quiet` in a repository
  without remotes
- `git maintenance run --task=prefetch` with a local path remote
- `git maintenance run --task=prefetch` with an unsupported remote-helper URL

The evidence compares stock Git and Zmin command output for no-remote and
unsupported-helper cases, and compares `refs/prefetch` plus `fsck --strict`
state after local-remote prefetch.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+3` closed rows, `+0` open rows, `+1` invalid-input row, `+3`
represented oracle functions, `-3` missing-or-unclassified oracle functions,
`+0` commands with rows and `+1` represented doc-option pair.

### Declared: Maintenance Run Local Tasks Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`, current largest missing bucket
`git_maintenance_compat.rs`, maintenance run local task and schedule behavior
group.

Evidence functions:

- `git_maintenance_compat::maintenance_run_gc_reuses_repository_gc`
- `git_maintenance_compat::maintenance_run_local_tasks_create_stock_readable_metadata`
- `git_maintenance_compat::maintenance_run_schedule_matches_stock_git_strategy_selection`
- `git_maintenance_compat::maintenance_run_daily_prunes_packed_loose_duplicates_like_stock_git`
- `git_maintenance_compat::maintenance_run_weekly_packs_refs_like_stock_git`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+5`
- missing-or-unclassified oracle functions: `-5`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `maintenance --task`,
  `--schedule` and `--quiet` already have matrix rows
- Rust behavior changes: no

Expected rows:

- `git maintenance run --task=gc --quiet`
- `git maintenance run --task=commit-graph --task=pack-refs --task=loose-objects`
- `git maintenance run --schedule=hourly`
- `git maintenance run --schedule=daily --quiet` with incremental strategy
  metadata creation
- `git maintenance run --schedule=daily --quiet` pruning duplicate loose
  objects
- `git maintenance run --schedule=weekly --quiet`

The evidence checks stock-readable repository state after local maintenance
tasks: pack creation and reachable `HEAD`, commit-graph verification, packed
refs readability, multi-pack-index verification, duplicate loose-object pruning
and weekly packed-ref side effects.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+5`
represented oracle functions, `-5` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

### Completed: Pull Local Rebase Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence file:

- `git_transport_local_compat.rs`

Evidence functions:

- `git_transport_local_compat::pull_local_remote_fast_forwards_like_stock_git`
- `git_transport_local_compat::pull_rebase_local_remote_replays_local_commit_like_stock_git`
- `git_transport_local_compat::pull_rebase_interactive_applies_sequence_editor_drop_like_stock_git`
- `git_transport_local_compat::pull_rebase_interactive_reword_uses_commit_editor_like_stock_git`
- `git_transport_local_compat::pull_rebase_interactive_edit_stops_and_continue_matches_stock_git`
- `git_transport_local_compat::pull_rebase_interactive_edit_abort_restores_original_head_like_stock_git`
- `git_transport_local_compat::pull_rebase_merges_local_remote_preserves_merge_topology_like_stock_git`
- `git_transport_local_compat::pull_rebase_config_replays_local_commit_like_stock_git`
- `git_transport_local_compat::pull_branch_rebase_config_replays_local_commit_like_stock_git`
- `git_transport_local_compat::pull_rebase_false_overrides_config_like_stock_git`

Expected movement:

- behavior rows: `+10`
- closed rows: `+10`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+10`
- missing-or-unclassified oracle functions: `-10`
- commands with rows: `+1`
- represented doc-option pairs: to be confirmed by generated summary because
  this creates the first `pull` matrix and maps command/subcommand options
  against the Git docs seed
- Rust behavior changes: no

Expected rows:

- `git pull --ff-only`
- `git pull --rebase`
- `GIT_SEQUENCE_EDITOR=<drop> git pull --rebase=interactive`
- `GIT_SEQUENCE_EDITOR=<reword> GIT_EDITOR=<rewrite> git pull --rebase=interactive`
- `GIT_SEQUENCE_EDITOR=<edit> git pull --rebase=interactive` with
  `git rebase --continue` verification after the stop
- `GIT_SEQUENCE_EDITOR=<edit> git pull --rebase=interactive` followed by
  `git rebase --abort`
- `git pull --rebase=merges`
- `git -c pull.rebase=true pull`
- `git -c branch.main.rebase=true pull`
- `git -c pull.rebase=true pull --rebase=false`

The evidence compares stock Git and Zmin command output, resulting `HEAD`,
tree contents, commit subjects, merge-parent counts, rebase control
directories and `status --porcelain=v1 --branch` output across local/file
pull workflows.

Actual post-import movement matched the declaration: `+10` behavior rows,
`+10` closed rows, `+0` open rows, `+0` invalid-input rows, `+10`
represented oracle functions, `-10` missing-or-unclassified oracle functions,
`+1` command with rows and `+3` represented doc-option pairs.

### Completed: Maintenance Prefetch Transport Batch

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence file:

- `git_transport_http_compat.rs`

Evidence functions:

- `git_transport_http_compat::maintenance_prefetch_reads_dumb_http_remote_like_stock_git`
- `git_transport_http_compat::maintenance_prefetch_reads_smart_http_remote_like_stock_git`
- `git_transport_http_compat::maintenance_prefetch_reads_git_daemon_remote_like_stock_git`
- `git_transport_http_compat::maintenance_prefetch_reads_ssh_remote_like_stock_git`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `maintenance --task` already
  has matrix rows
- Rust behavior changes: no

Expected rows:

- `git maintenance run --task=prefetch` with a dumb HTTP remote
- `git maintenance run --task=prefetch` with a smart HTTP remote
- `git maintenance run --task=prefetch` with a git-daemon remote
- `GIT_SSH_COMMAND=<fake-ssh> git maintenance run --task=prefetch` with an
  SSH remote

The evidence compares stock Git and Zmin `refs/prefetch` refnames/object IDs
and fetched object contents after maintenance prefetch over each transport.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+4`
represented oracle functions, `-4` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_resolves_unmerged_entries_preserving_disabled_mode_bits_like_stock_git`
- `git_index_mutation_compat::add_update_matches_stock_git_state`
- `git_index_mutation_compat::add_rejects_submodule_object_format_mismatch_like_upstream_git`

Expected movement:

- behavior rows: `+4`
- matching stock Git rows: `+2`
- open rows: `+0`
- invalid-input rows: `+2`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`
- Rust behavior changes: no

Expected rows:

- `git add file symlink` with `core.filemode=false`,
  `core.symlinks=false` and unmerged file/symlink index stages
- `git add -u dir`
- `git add submodule` in a sha256 parent with a sha1 submodule
- `git add submodule` in a sha1 parent with a sha256 submodule

The evidence compares stock Git and Zmin index or status output for the
unmerged and update rows, and checks the upstream Git object-format mismatch
diagnostic shape for invalid submodule additions.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::rm_file_dir_and_cached_match_stock_git_state`
- `git_index_mutation_compat::rm_cached_recursive_root_pathspec_matches_stock_git`
- `git_index_mutation_compat::rm_common_options_match_stock_git`

Expected movement:

- behavior rows: `+9`
- matching stock Git rows: `+8`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+6` for `rm -r`, `rm -n`,
  `rm -q`, `rm --ignore-unmatch`, `rm --pathspec-from-file` and
  `rm --pathspec-file-nul`
- Rust behavior changes: no

Expected rows:

- `git rm a.txt`
- `git rm -r dir`
- `git rm --cached cached.txt`
- `git rm --cached -r .`
- `git rm -n a.txt`
- `git rm -q a.txt`
- `git rm --ignore-unmatch missing.txt`
- `git rm --cached --pathspec-from-file paths.nul --pathspec-file-nul`
- `git rm --pathspec-file-nul`

The evidence compares stock Git and Zmin stdout/stderr, exit status, status
output, `ls-files` output and worktree side effects for file, directory,
cached, dry-run, quiet, ignore-unmatch and pathspec-file modes.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_autocrlf_warning_and_blob_normalization_match_stock_git`
- `git_index_mutation_compat::add_autocrlf_mixed_eol_over_binary_index_matches_stock_git`
- `git_index_mutation_compat::add_ignore_errors_stages_readable_siblings_like_stock_git`
- `git_index_mutation_compat::add_ignore_errors_config_stages_readable_siblings_like_stock_git`

Expected movement:

- behavior rows: `+5`
- matching stock Git rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `add --ignore-errors`
- Rust behavior changes: no

Expected rows:

- `git -c core.autocrlf=true add LF`
- `git -c core.autocrlf=input add CRLF`
- `git -c core.autocrlf=true add file.txt` after a binary-index path is
  rewritten with mixed line endings
- `git add --ignore-errors .` with a readable and unreadable sibling
- `git add .` with `add.ignore-errors=true`

The evidence compares stock Git and Zmin autocrlf output, staged blob
normalization, status output and index contents for line-ending and
ignore-errors add behavior.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_escaped_bracket_pathspec_matches_literal_path_like_stock_git`
- `git_index_mutation_compat::add_resolves_conflict_on_ignored_path_like_stock_git`
- `git_index_mutation_compat::add_embedded_repository_warning_matches_stock_git`
- `git_index_mutation_compat::add_empty_embedded_repository_error_matches_stock_git`

Expected movement:

- behavior rows: `+4`
- matching stock Git rows: `+3`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`
- Rust behavior changes: no

Expected rows:

- `git add 'fo\[ou\]bar'`
- `git add track-this` resolving an unmerged ignored path
- `git add .` with embedded repositories that have commits
- `git add empty` with an embedded repository that has no checked-out commit

The evidence compares stock Git and Zmin pathspec handling, index entries,
stdout/stderr, exit status and failure diagnostics for index mutation edge
cases.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_chmod_stages_mode_like_stock_git`
- `git_index_mutation_compat::add_chmod_dry_run_and_symlink_errors_match_stock_git`
- `git_index_mutation_compat::add_chmod_stages_regular_paths_when_non_regular_path_fails_like_stock_git`
- `git_index_mutation_compat::add_chmod_rejects_index_symlink_even_when_worktree_path_is_regular_like_stock_git`

Expected movement:

- behavior rows: `+6`
- matching stock Git rows: `+3`
- open rows: `+0`
- invalid-input rows: `+3`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `add --chmod`
- Rust behavior changes: no

Expected rows:

- `git add --chmod=+x foo`
- `git add --chmod=-x foo`
- `git add --chmod=+x --dry-run foo`
- `git add --chmod=+x --dry-run link` for a symlink path
- `git add --chmod=+x link regular` with a symlink and a regular path
- `git add --chmod=+x link regular` when `core.symlinks=false` but the index
  entry remains a symlink

The evidence compares stock Git and Zmin index entries, dry-run output, exit
status and failure diagnostics for chmod staging and non-regular path
rejection behavior.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_dry_run_reports_without_mutating_index_like_stock_git`
- `git_index_mutation_compat::add_dry_run_allows_tracked_ignored_path_like_stock_git`
- `git_index_mutation_compat::add_dry_run_ignore_missing_reports_tracked_and_ignored_like_stock_git`

Expected movement:

- behavior rows: `+3`
- matching stock Git rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `add --dry-run`
- Rust behavior changes: no

Expected rows:

- `git add --dry-run track-this` for a new file without mutating the index
- `git add --dry-run track-this` for a tracked path now ignored by `.gitignore`
- `git add --dry-run --ignore-missing track-this ignored-file`

The evidence compares stock Git and Zmin dry-run output, exit status and
index side effects where observable.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_all_stages_mode_change_with_unchanged_content_like_stock_git`
- `git_index_mutation_compat::add_preserves_index_symlink_mode_when_core_symlinks_false_like_stock_git`
- `git_index_mutation_compat::add_refresh_updates_stat_after_read_tree_like_stock_git`
- `git_index_mutation_compat::add_refresh_pathspec_leaves_other_stat_dirty_paths_like_stock_git`
- `git_index_mutation_compat::add_all_stages_same_size_rewrite_after_reset_like_stock_git`
- `git_index_mutation_compat::add_refresh_reports_unmatched_pathspec_like_stock_git`

Expected movement:

- behavior rows: `+6`
- matching stock Git rows: `+5`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `add --refresh`
- Rust behavior changes: no

Expected rows:

- `git add -A` after an executable-mode-only worktree change
- `git add xfoo1` with `core.symlinks=false` preserving an index symlink mode
- `git add --refresh -- foo` after `read-tree HEAD`
- `git add --refresh bar` leaving another stat-dirty path dirty
- `git add -A` after reset and same-size tracked rewrite
- `git add --refresh nonexistent` as stock-compatible invalid input

The evidence compares stock Git and Zmin index entries, cached diffs,
diff-index/diff-files output or invalid-pathspec exit status and stderr.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_index_mutation_compat::add_all_pathspec_limits_tracked_deletes_like_stock_git`
- `git_index_mutation_compat::add_force_stages_explicit_ignored_paths_like_stock_git`
- `git_index_mutation_compat::add_rejects_explicit_ignored_paths_without_force_like_stock_git`
- `git_index_mutation_compat::add_honors_nested_gitignore_negation_like_stock_git`
- `git_index_mutation_compat::add_respects_core_filemode_false_like_stock_git`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+5`
- missing-or-unclassified oracle functions: `-5`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `add -A` and `add -f`
- Rust behavior changes: no

Expected rows:

- `git add -A dir` after deleting tracked paths inside and outside `dir`
- `git add -f ignored.txt` for an explicitly ignored path
- `git add a.if a.ig` where one explicit path is ignored and rejected
- `git add sub/dir` with nested `.gitignore` negation
- `git -c core.filemode=false add script.sh` for an executable worktree file

The evidence compares stock Git and Zmin status, index entries, cached diffs or
command exit status for index mutation behavior.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_notes_compat::notes_edit_matches_stock_git_for_update_and_empty_remove`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; notes subcommand option rows
  are not represented in the current Git docs option seed
- Rust behavior changes: no

Expected rows:

- `GIT_EDITOR=edit-note.sh git notes edit HEAD` updates an existing note
- `GIT_EDITOR=edit-note.sh git notes edit HEAD` empties the note file and
  removes the note

The evidence compares stock Git and Zmin command output plus resulting note
contents/status for editor-backed update and empty-result removal flows.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_notes_compat::notes_reuse_message_matches_stock_git_for_add_and_append`
- `git_notes_compat::notes_reedit_message_matches_stock_git_for_add_and_append`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; notes subcommand option rows
  are not represented in the current Git docs option seed
- Rust behavior changes: no

Expected rows:

- `git notes add -m literal -C <blob> HEAD`
- `git notes append --reuse-message <blob> HEAD`
- `GIT_EDITOR=reedit-note.sh git notes add -c <blob> HEAD`
- `GIT_EDITOR=reedit-note.sh git notes append --reedit-message <blob> HEAD`

The evidence compares stock Git and Zmin command output plus resulting note
contents for reuse-message and reedit-message add/append flows.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_notes_compat::notes_copy_stdin_matches_stock_git_for_pair_stream`
- `git_notes_compat::notes_copy_stdin_no_stdin_toggles_match_stock_git_order`
- `git_notes_compat::notes_copy_for_rewrite_matches_stock_git_config_gate`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; notes subcommand option rows
  are not represented in the current Git docs option seed
- Rust behavior changes: no

Expected rows:

- `git notes copy --stdin` with an object-pair stream
- `git notes copy --stdin --no-stdin <from> <to>`
- `git notes copy --no-stdin --stdin` with an object-pair stream
- `git notes copy --for-rewrite=rebase` with rewrite ref config
- `git notes copy --for-rewrite rebase` with rewrite disabled by config

The evidence compares stock Git and Zmin command output plus destination note
contents or absence for stdin, no-stdin and for-rewrite copy flows.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows, `+3`
represented oracle functions, `-3` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_trailers_match_stock_git_object_and_editor_buffer`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: `+2`
- Rust behavior changes: no

Expected rows:

- `git commit -m subject --trailer ...`
- `git commit -F message.txt --trailer ...`
- `git commit -m subject --trailer ... --trailer ...`
- `git commit --signoff -m subject --trailer ...`
- editor-backed `git commit --trailer ...`

The evidence compares stock Git and Zmin commit objects, command output and
editor input buffers for trailer insertion flows.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+2` represented doc-option pairs.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_cleanup_modes_match_stock_git_object`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit --cleanup strip -F message.txt`
- `git commit --cleanup whitespace -F message.txt`
- `git commit --cleanup default -F message.txt`
- `git commit --no-cleanup -F message.txt`

The evidence compares stock Git and Zmin commit objects for each cleanup mode.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+1` represented doc-option pair.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_summary_and_quiet_output_match_stock_git`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -m initial` root commit summary output
- `git commit -m second` update summary output
- `git commit -m delete` deletion summary output
- `git commit --quiet -m quiet`

The evidence compares stock Git and Zmin stdout/stderr/exit behavior for root,
update, deletion and quiet commit flows.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+1` represented doc-option pair.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_edit_and_no_edit_message_sources_match_stock_git`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -e -m "Msg subject" -m "Msg body"`
- `git commit -e -F msg.txt`
- `git commit -e -C HEAD`
- `git commit --no-edit -c HEAD`
- `git commit --no-edit -m "No edit subject"`

The evidence compares stock Git and Zmin commit objects, command output,
editor invocation and editor input buffers for edit/no-edit message-source
flows.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+2` represented doc-option pairs.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_reuse_message_matches_stock_git_object`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -C HEAD`
- `git commit -C HEAD~1 --author ... --date ...`
- `git commit -c HEAD~2`
- `git commit -c HEAD~3`

The evidence compares stock Git and Zmin commit objects, command output and
editor input buffers for reuse and reedit message flows.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function and
`+0` commands with rows.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_messages_match_stock_git_object`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -m subject -m body`
- `git commit -F message.txt`
- `git commit --allow-empty-message -m ""`
- `git commit --amend -m amended -m details`

The evidence compares stock Git and Zmin commit objects for multiple `-m`
paragraphs, file-backed messages, empty-message commits and amend commits with
multiple message paragraphs.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function and
`+0` commands with rows.

## Earlier Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_pathspec_only_and_fixup_match_stock_git_state`

Expected movement:

- behavior rows: `+7`
- closed rows: `+7`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git commit -m base`
- `git commit -m path -- a.txt`
- `git commit --only -m "only path" a.txt`
- `git commit --fixup HEAD`
- `git commit --fixup HEAD -m "fixup detail"`
- `git commit --fixup=amend:HEAD`
- `git commit --fixup=reword:HEAD`

The evidence compares stock Git and Zmin commit objects, command output,
editor input buffers, porcelain status and index state for pathspec-only and
autosquash fixup commit flows.

Actual post-import movement matched the declaration: `+7` behavior rows,
`+7` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function and
`+0` commands with rows.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_commit_compat::commit_amend_matches_stock_git_state`
- `git_commit_compat::commit_dot_pathspec_matches_stock_git_state`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+1`
- Rust behavior changes: no

Expected rows:

- `git commit -m initial`
- `git commit --amend -m amended`
- `git commit --amend -m message only`
- `git commit --amend --no-edit`
- `git commit --amend` with `GIT_EDITOR=:`
- `git commit -m "add attrs" .`

The evidence compares stock Git and Zmin commit objects, parent shape, commit
count, porcelain status and index state for amend and dot-pathspec commit
flows. Broader pathspec/fixup/editor rows from the same test file stay
unimported until a separate batch.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions
and `+1` command with rows.

## Previous Inventory Classification Fix

Source bucket: focused stock-oracle inventory tooling for evidence cells that
already contain multiple test references.

The previous inventory script treated an entire TSV evidence cell as one key.
Rows such as
`git_status_compat::status_porcelain_matches_stock_git_for_clean_dirty_and_ignored_worktrees; git_status_compat::status_porcelain_v2_matches_stock_git_for_staged_states`
therefore failed to credit the second evidence function. The inventory now
extracts every `module::test` reference from matrix rows and classification
docs.

Expected movement:

- behavior rows: `+0`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- missing-or-unclassified oracle functions: `-3`
- Rust behavior changes: no

Reclassified functions:

- `git_global_cli_compat::rev_parse_show_prefix_inside_git_dir_matches_stock_git`
- `git_global_cli_compat::rev_parse_core_bare_config_affects_discovery_flags`
- `git_status_compat::status_porcelain_v2_matches_stock_git_for_staged_states`

Actual post-fix movement matched the declaration: inventory represented
functions moved from `500` to `503`, and `missing_or_unclassified` moved from
`461` to `458`. Written behavior rows stayed `2354`.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:
`git_stash_compat::stash_apply_pop_branch_reject_too_many_refs_like_stock_git`
in `crates/zmin-cli/tests/git_stash_compat.rs`.

Expected delta:

- behavior rows: `+4`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+4`
- represented oracle functions: `+1`
- Rust behavior changes: no

Expected rows:

- `git stash apply stash@{0} stash@{1}`
- `git stash pop stash@{0} stash@{1}`
- `git stash show stash@{0} stash@{1}`
- `git stash branch stash-branch stash@{0} stash@{1}`

The evidence compares stock Git and Zmin failure output and verifies the dirty
worktree file is unchanged after each rejected invocation.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+0` closed rows, `+0` open rows, `+4` invalid-input rows and `+1`
represented oracle function.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:
`git_stash_compat::stash_push_apply_pop_matches_stock_git_state` in
`crates/zmin-cli/tests/git_stash_compat.rs`.

Expected delta:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- Rust behavior changes: no

Expected rows:

- `git stash push -m work`
- `git stash list`
- `git stash clear`
- `git stash apply`
- `git stash pop`

The evidence compares stock Git and Zmin for the default tracked-change stash
flow, including clean worktree/index after push, stash list/rev side effects,
clearing the stack, applying a stash without dropping it and popping a stash
while emptying the stack.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows and `+1`
represented oracle function.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:
`git_stash_compat::stash_pop_drops_selected_stack_entry_like_stock_git` in
`crates/zmin-cli/tests/git_stash_compat.rs`.

Expected delta:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- Rust behavior changes: no

Expected rows:

- `git stash pop stash@{1}`
- `git stash push --message=custom --no-message`
- `git stash push --no-message --message=after-no`
- `git stash push -m no-pathspec-file --pathspec-from-file=paths.txt --no-pathspec-from-file`
- `git stash push -m no-pathspec-nul --pathspec-file-nul --no-pathspec-file-nul --pathspec-from-file=paths.txt`

The evidence compares stock Git and Zmin for selected stash-pop stack updates,
last-option-wins message/no-message push behavior, and negated pathspec file
mode behavior while checking worktree status and stash show output against
stock Git.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows and `+1`
represented oracle function.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_stash_compat.rs`:

- `git_stash_compat::stash_preserves_missing_skip_worktree_entry_like_stock_git`
- `git_stash_compat::stash_uses_git_stash_identity_when_user_identity_is_missing_like_stock_git`
- `git_stash_compat::stash_push_captures_same_size_rewrite_after_reset_like_stock_git`
- `git_stash_compat::stash_pop_rejects_overlapping_dirty_paths_like_stock_git`
- `git_stash_compat::stash_apply_and_drop_selected_stack_entry_match_stock_git`
- `git_stash_compat::stash_save_rm_then_recreate_matches_stock_git`
- `git_stash_compat::stash_save_file_to_directory_matches_stock_git`

Expected delta:

- behavior rows: `+7`
- closed rows: `+7`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+7`
- Rust behavior changes: no

Expected rows:

- `git stash` with a missing skip-worktree tracked entry
- `git stash` without configured user identity, using `git stash <git@stash>`
- `git stash; git stash drop stash@{1}; git stash apply` after same-size rewrites
- `git stash pop` rejected by an overlapping dirty path
- `git stash apply stash@{1}; git stash drop 1`
- `git stash save "rm then recreate"; git stash apply`
- `git stash save "file to directory"; git stash apply`

The evidence compares stock Git and Zmin for stash commit contents, fallback
stash identity, repeated rewrite capture, dirty-path rejection, selected stack
entry operations and legacy `save` path-shape restoration.

Actual post-import movement matched the declaration: `+7` behavior rows,
`+7` closed rows, `+0` open rows, `+0` invalid-input rows and `+7`
represented oracle functions.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_stash_compat.rs`:

- `git_stash_compat::stash_rejects_intent_to_add_entries_like_stock_git`
- `git_stash_compat::stash_patch_selects_hunks_and_leaves_rejected_hunks_like_stock_git`
- `git_stash_compat::stash_patch_all_done_quit_and_pathspec_match_stock_git`
- `git_stash_compat::stash_patch_split_pathspec_restores_selected_hunk_like_stock_git`

Expected delta:

- behavior rows: `+6`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+4`
- Rust behavior changes: no

Expected rows:

- `git add --intent-to-add file4; git stash`
- `printf 'y\nn\n' | git stash push --patch -m patchy`
- `printf 'a\n' | git stash push --patch -m all-a -- a.txt`
- `printf 'd\n' | git stash push --patch -m done`
- `printf 'q\ny\n' | git stash push --patch -m quit`
- `printf 's\ny\nn\n' | git stash push -m "stash bar" --patch file`

The evidence compares stock Git and Zmin for intent-to-add rejection,
interactive patch hunk selection, pathspec-limited all selection, done/quit
commands, split hunk selection, stash list/show output, exit status and
worktree file contents.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+5` closed rows, `+0` open rows, `+1` invalid-input row and `+4`
represented oracle functions.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_reflog_compat.rs`:

- `git_reflog_compat::reflog_show_list_and_exists_match_stock_git`
- `git_reflog_compat::reflog_show_date_modes_match_stock_git`
- `git_reflog_compat::reflog_show_passes_pathspec_after_double_dash`

Expected delta:

- behavior rows: `+7`
- closed rows: `+7`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- Rust behavior changes: no

Expected rows:

- `git reflog`
- `git reflog show refs/heads/main --format=%H`
- `git reflog list`
- `git reflog exists HEAD`
- `GIT_TEST_DATE_NOW=1700003600 git reflog --date=<common-mode>`
- `git reflog show -- --does-not-exist`
- `git reflog show -- --a-file`

The evidence compares stock Git and Zmin for default reflog show output,
formatted ref output, reflog listing, exists exit status, date rendering under
a fixed oracle clock and double-dash pathspec filtering.

Actual post-import movement matched the declaration: `+7` behavior rows,
`+7` closed rows, `+0` open rows, `+0` invalid-input rows and `+3`
represented oracle functions.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_reflog_compat.rs`:

- `git_reflog_compat::reset_hard_records_branch_and_head_reflog`
- `git_reflog_compat::commit_records_branch_and_head_reflog`
- `git_reflog_compat::branch_create_reflog_handles_nested_branch_names`
- `git_reflog_compat::log_reflog_orphan_checkout_uses_contiguous_commit_ordinals`

Expected delta:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- Rust behavior changes: no

Expected rows:

- `git reset --hard HEAD~1; git reflog show main`
- `git commit --allow-empty -m one; git commit --allow-empty -m two; git reflog show main`
- `git branch one/two main; git log -g --format=%gd\ %gs one/two`
- `git log -g --format=%gd\ %gs HEAD` after orphan checkout commits

The evidence compares stock Git and Zmin for reflog entries produced by reset,
commit and branch creation plus `log -g` ordinal display after orphan-checkout
reflog entries. The hidden zero-oid reflog row from
`git_reflog_compat::log_reflog_keeps_ordinals_for_hidden_zero_oid_entries` was
already represented in `log_v2_47.tsv`, so it is intentionally not counted in
this import batch.

Actual post-import movement matched the corrected declaration: `+4` behavior
rows, `+4` closed rows, `+0` open rows, `+0` invalid-input rows and `+4`
represented oracle functions.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_reflog_compat.rs`:

- `git_reflog_compat::reflog_expire_dry_run_does_not_touch_reflog`
- `git_reflog_compat::reflog_expire_default_current_entries_match_stock_git`
- `git_reflog_compat::reflog_expire_pattern_config_matches_stock_git`

Expected delta:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+3`
- Rust behavior changes: no

Expected rows:

- `git reflog expire --dry-run main`
- `git reflog expire`, `git reflog expire main`, `git reflog expire HEAD`,
  `git reflog expire --updateref main`, `git reflog expire --rewrite main`
  and `git reflog expire --verbose main`
- `git reflog expire root1/branch1 root1/branch2 root2/branch1 root2/branch2`
  with per-pattern `gc.<pattern>.reflogExpire` config

The evidence compares stock Git and Zmin for `reflog expire` stdout/stderr,
exit status, resulting HEAD/main reflogs, dry-run non-mutation and per-pattern
expiry config side effects. The dry-run focused test was tightened in this
slice to compare stock and Zmin command output directly before counting the
row as represented.

Actual post-import movement matched the declaration: `+3` behavior rows,
`+3` closed rows, `+0` open rows, `+0` invalid-input rows and `+3`
represented oracle functions.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_pack_integrity_compat.rs`:

- `git_pack_integrity_compat::fsck_missing_email_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_email_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_author_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_committer_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_tagger_entry_severity_config_matches_stock_git`

Expected delta:

- behavior rows: `+5`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+5`
- represented oracle functions: `+5`
- commands with rows: `+1`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.missingEmail=bogus fsck`
- `git -c fsck.badEmail=bogus fsck`
- `git -c fsck.missingAuthor=bogus fsck`
- `git -c fsck.missingCommitter=bogus fsck`
- `git -c fsck.missingTaggerEntry=bogus fsck`

The evidence compares stock Git and Zmin stdout/stderr and exit status for
invalid `fsck.<message>` severity config values against malformed commit and
tag objects. The accepted severity values from the same tests are not counted
in this batch; they need separate closed rows to avoid mixing accepted and
invalid-input statuses.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+0` closed rows, `+0` open rows, `+5` invalid-input rows, `+5` represented
oracle functions and `+1` command with rows.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_pack_integrity_compat.rs`:

- `git_pack_integrity_compat::fsck_bad_tag_name_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_date_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_email_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_email_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_name_before_email_tagger_severity_config_matches_stock_git`

Expected delta:

- behavior rows: `+5`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+5`
- represented oracle functions: `+5`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.badTagName=bogus fsck`
- `git -c fsck.badDate=bogus fsck`
- `git -c fsck.missingEmail=bogus fsck` for a malformed tagger identity
- `git -c fsck.badEmail=bogus fsck` for a malformed tagger identity
- `git -c fsck.missingNameBeforeEmail=bogus fsck`

The evidence compares stock Git and Zmin stdout/stderr and exit status for
invalid `fsck.<message>` severity config values against malformed tag objects.
Accepted severity values from the same tests remain uncounted until they get
separate closed rows.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+0` closed rows, `+0` open rows, `+5` invalid-input rows, `+5` represented
oracle functions and `+0` commands with rows.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_pack_integrity_compat.rs`:

- `git_pack_integrity_compat::fsck_missing_space_before_email_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_missing_space_before_date_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_zero_padded_date_tagger_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_date_severity_config_matches_stock_git`
- `git_pack_integrity_compat::fsck_bad_timezone_severity_config_matches_stock_git`

Expected delta:

- behavior rows: `+5`
- closed rows: `+0`
- open rows: `+0`
- invalid-input rows: `+5`
- represented oracle functions: `+5`
- commands with rows: `+0`
- Rust behavior changes: no

Expected rows:

- `git -c fsck.missingSpaceBeforeEmail=bogus fsck`
- `git -c fsck.missingSpaceBeforeDate=bogus fsck`
- `git -c fsck.zeroPaddedDate=bogus fsck`
- `git -c fsck.badDate=bogus fsck` for a malformed author date
- `git -c fsck.badTimezone=bogus fsck`

The evidence compares stock Git and Zmin stdout/stderr and exit status for
invalid `fsck.<message>` severity config values against malformed tagger date,
tagger identity, author date and author timezone objects. Accepted severity
values from the same tests remain uncounted until they get separate closed
rows.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+0` closed rows, `+0` open rows, `+5` invalid-input rows, `+5` represented
oracle functions and `+0` commands with rows.

## Previous Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_commit_compat::commit_status_option_and_config_match_stock_git_buffer`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+3` if `--status`, `--no-status`
  and `-v` were not represented for `commit`
- Rust behavior changes: no

Expected rows:

- `GIT_EDITOR=.git/editor.sh git commit --no-status`
- `GIT_EDITOR=.git/editor.sh git commit` with `commit.status=false`
- `GIT_EDITOR=.git/editor.sh git commit --status` with
  `commit.status=false`
- `GIT_EDITOR=.git/editor.sh git commit -v --no-status`

The evidence compares stock Git and Zmin command output, captured editor
buffer and resulting commit object for status-template suppression, explicit
status override, config-driven status suppression and verbose/no-status
combination behavior.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+3` represented doc-option pairs.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_commit_compat.rs`:

- `git_commit_compat::commit_template_editor_matches_stock_git_object`
- `git_commit_compat::commit_editor_without_message_matches_stock_git_buffer`
- `git_commit_compat::commit_editor_empty_and_unchanged_abort_like_stock_git`
- `git_commit_compat::commit_verbose_template_editor_matches_stock_git_buffer`
- `git_commit_compat::commit_verbose_verbose_template_editor_matches_stock_git_buffer`
- `git_commit_compat::commit_template_requires_editor_change_like_stock_git`

Expected movement:

- behavior rows: `+8`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+3`
- represented oracle functions: `+6`
- missing-or-unclassified oracle functions: `-6`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `--template`; `-vv` is a
  value/spelling form of the existing verbose option seed rather than a
  separate documented option pair
- Rust behavior changes: no

Expected rows:

- `GIT_EDITOR=./editor.sh git commit --template template.txt`
- `GIT_EDITOR=./editor.sh git commit` with `commit.template` configured
- `GIT_EDITOR=.git/editor.sh git commit`
- `GIT_EDITOR=.git/editor.sh git commit` with unchanged editor buffer
- `GIT_EDITOR=.git/editor.sh git commit` with empty editor buffer
- `GIT_EDITOR=.git/editor.sh git commit --template .git/template.txt -v`
- `GIT_EDITOR=.git/editor.sh git commit --template .git/template.txt -vv`
- `GIT_EDITOR=true git commit --template template.txt`

The evidence compares stock Git and Zmin command output, captured editor
buffers, resulting commit objects and failure output for editor-backed commits,
template messages, verbose template buffers and unchanged or empty editor
abort behavior.

Actual post-import movement matched the declaration: `+8` behavior rows,
`+5` closed rows, `+0` open rows, `+3` invalid-input rows, `+6`
represented oracle functions, `-6` missing-or-unclassified oracle functions,
`+0` commands with rows and `+1` represented doc-option pair.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_commit_compat.rs`:

- `git_commit_compat::commit_long_message_option_matches_stock_git_object`
- `git_commit_compat::commit_author_matches_stock_git_object`
- `git_commit_compat::commit_date_matches_stock_git_object`
- `git_commit_compat::commit_amend_author_date_options_match_stock_git_object`
- `git_commit_compat::commit_reset_author_matches_stock_git_object`
- `git_commit_compat::commit_signoff_matches_stock_git_object`
- `git_commit_compat::commit_squash_matches_stock_git_object`

Expected movement:

- behavior rows: `+8`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+7`
- missing-or-unclassified oracle functions: `-7`
- commands with rows: `+0`
- represented doc-option pairs: expected up to `+5` for `--message`,
  `--author`, `--date`, `--reset-author` and `--squash`
- Rust behavior changes: no

Expected rows:

- `git commit --allow-empty --message empty`
- `git commit --author "Alice Example <alice@example.test>" -m author`
- `git commit --date "1700001234 +0000" -m date`
- `git commit --amend --author "Alice Example <alice@example.test>" --date "1700001234 +0000" -m amended`
- `git commit --amend --reset-author -m reset`
- `git commit --allow-empty-message --signoff -m ""`
- `git commit --signoff -m subject -m details`
- `git commit --squash HEAD~1 -m work`

The evidence compares stock Git and Zmin commit objects and, for the author
row, log-rendered author identity for long message spelling, explicit
author/date metadata, amend metadata overrides, reset-author behavior, signoff
messages and squash messages.

Actual post-import movement matched the declaration: `+8` behavior rows,
`+8` closed rows, `+0` open rows, `+0` invalid-input rows, `+7`
represented oracle functions, `-7` missing-or-unclassified oracle functions,
`+0` commands with rows and `+5` represented doc-option pairs.

## Previous Declared Import

Source bucket: focused stock-oracle tests already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions in `crates/zmin-cli/tests/git_commit_compat.rs`:

- `git_commit_compat::commit_hooks_match_stock_git_flow`
- `git_commit_compat::commit_prepare_and_post_rewrite_hooks_match_stock_git_flow`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` if `--no-verify` was not
  represented for `commit`
- Rust behavior changes: no

Expected rows:

- `git commit -m subject` with successful `pre-commit`, `commit-msg` and
  `post-commit` hooks
- `git commit --no-verify -m skip` with verification hooks skipped
- `git commit --no-verify -m subject` with `prepare-commit-msg` still running
- `git commit --amend -m amended` with `post-rewrite` running

The evidence compares stock Git and Zmin command output, hook logs and commit
objects for successful hook execution, no-verify behavior,
prepare-commit-msg and post-rewrite amend behavior.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+2`
represented oracle functions, `-2` missing-or-unclassified oracle functions,
`+0` commands with rows and `+1` represented doc-option pair.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_transport_local_compat::ls_remote_matches_stock_git_for_local_remotes`
- `git_transport_local_compat::ls_remote_reads_local_gitfile_repository_like_stock_git`

Expected movement:

- behavior rows: `+8`
- closed rows: `+8`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+2`
- missing-or-unclassified oracle functions: `-2`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `ls-remote` repository,
  `--heads`, `--tags` and `--refs` already have rows
- Rust behavior changes: no

Expected rows:

- `git ls-remote origin`
- `git ls-remote --heads origin`
- `git ls-remote --tags origin`
- `git ls-remote --refs origin`
- `git ls-remote origin main v*`
- `git ls-remote <local-bare-path>`
- `git ls-remote file://<local-bare-path>`
- `git ls-remote <local-gitfile-worktree>`

The evidence compares stock Git and Zmin stdout for named local remotes,
direct local paths, file URLs, multi-pattern filtering and repositories whose
`.git` entry is a gitfile pointing to the real Git directory.

Actual post-import movement matched the declaration: `+8` behavior rows, `+8`
closed rows, `+0` open rows, `+0` invalid-input rows, `+2` represented oracle
functions, `-2` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_worktree_state_compat::checkout_dot_pathspec_restores_root_like_stock_git`
- `git_worktree_state_compat::checkout_dot_reports_updated_paths_like_stock_git`
- `git_worktree_state_compat::checkout_separator_pathspec_omits_updated_paths_like_stock_git`
- `git_worktree_state_compat::checkout_recurse_submodules_flag_keeps_dot_pathspec_like_stock_git`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1`; checkout pathspec rows now
  represent one more documented option pair
- Rust behavior changes: no

Expected rows:

- `git checkout --quiet --no-progress .`
- `git checkout .`
- `git checkout -- a.txt`
- `git checkout --recurse-submodules .`

The evidence compares stock Git and Zmin worktree restoration and updated-path
reporting for dot pathspec checkout, separator pathspec checkout and
`--recurse-submodules` dot pathspec handling.

Actual post-import movement matched the declaration: `+4` behavior rows, `+4`
closed rows, `+0` open rows, `+0` invalid-input rows, `+4` represented oracle
functions, `-4` missing-or-unclassified oracle functions, `+0` commands with
rows and `+1` represented doc-option pair.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence functions:

- `git_transport_http_compat::http_backend_info_refs_matches_stock_git_smart_discovery_refs`
- `git_transport_http_compat::http_backend_resolves_scriptalias_path_translated_like_stock_git`
- `git_transport_http_compat::http_backend_serves_scriptalias_non_bare_repo_like_stock_git`
- `git_transport_http_compat::http_backend_upload_pack_post_returns_stock_readable_pack`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+4`
- missing-or-unclassified oracle functions: `-4`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `http-backend` already has
  represented doc-option rows
- Rust behavior changes: no

Expected rows:

- `GIT_PROJECT_ROOT=<root> PATH_INFO=/remote.git/info/refs?service=git-upload-pack git http-backend`
- `GIT_PROJECT_ROOT=<root> PATH_TRANSLATED=<root>/remote.git/info/refs git http-backend`
- `GIT_PROJECT_ROOT=<root> PATH_TRANSLATED=<root>/server/info/refs git http-backend`
- `GIT_PROJECT_ROOT=<root> PATH_INFO=/remote.git/git-upload-pack REQUEST_METHOD=POST git http-backend`

The evidence compares stock Git and Zmin smart HTTP advertised ref lines for
normal and ScriptAlias discovery paths, and verifies that the upload-pack POST
response includes stock-readable sideband pack data, acknowledges the common
`have`, includes delta objects and omits common base objects.

Actual post-import movement matched the declaration: `+4` behavior rows, `+4`
closed rows, `+0` open rows, `+0` invalid-input rows, `+4` represented oracle
functions, `-4` missing-or-unclassified oracle functions, `+0` commands with
rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_notes_compat::notes_add_list_show_remove_match_stock_git`

Expected movement:

- behavior rows: `+10`
- closed rows: `+9`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: to be confirmed by generated summary because
  `notes` subcommands and options are seeded separately
- Rust behavior changes: no

Expected rows:

- `git notes add -m "note text" HEAD`
- `git notes get-ref`
- `git notes list`
- `git notes show HEAD`
- `git notes remove HEAD`
- `git notes show HEAD` after note removal
- `git notes add -F note.txt HEAD`
- `git notes append -m appended HEAD`
- `git notes append -F append-note.txt HEAD`
- `git notes copy HEAD~1 HEAD`

The evidence compares stock Git and Zmin notes ref output, list/show output,
remove behavior, missing-note failure status, file-backed add, message and
file-backed append, and copy behavior across two commits.

Actual post-import movement matched the declaration: `+10` behavior rows,
`+9` closed rows, `+0` open rows, `+1` invalid-input row, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_notes_compat::notes_edit_message_source_options_match_stock_git`

Expected movement:

- behavior rows: `+12`
- closed rows: `+12`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; notes subcommand option rows
  are not represented in the current Git docs option seed
- Rust behavior changes: no

Expected rows:

- `git notes edit -m msg HEAD`
- `git notes edit --message=long HEAD`
- `git notes edit -mcompact HEAD`
- `git notes edit -F note.txt HEAD`
- `git notes edit --file=note.txt HEAD`
- `git notes edit -Fnote.txt HEAD`
- `git notes edit -C <blob> HEAD`
- `git notes edit --reuse-message=<blob> HEAD`
- `git notes edit -C<blob> HEAD`
- `GIT_EDITOR=edit-source-note.sh git notes edit -c <blob> HEAD`
- `GIT_EDITOR=edit-source-note.sh git notes edit --reedit-message=<blob> HEAD`
- `GIT_EDITOR=edit-source-note.sh git notes edit -c<blob> HEAD`

The evidence compares stock Git and Zmin command output and resulting note
contents for short, long and compact message, file, reuse-message and
reedit-message forms.

Actual post-import movement matched the declaration: `+12` behavior rows,
`+12` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_notes_compat::notes_allow_empty_matches_stock_git_for_add_and_append`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; notes subcommand option rows
  are not represented in the current Git docs option seed
- Rust behavior changes: no

Expected rows:

- `GIT_EDITOR=true git notes add --allow-empty HEAD`
- `GIT_EDITOR=true git notes append --allow-empty HEAD` without an existing
  note
- `GIT_EDITOR=true git notes append --allow-empty HEAD` with an existing note

The evidence compares stock Git and Zmin command output plus resulting note
contents for allow-empty add and append flows.

Actual post-import movement matched the declaration: `+3` behavior rows,
`+3` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already represented in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_mail_tools_compat::mailinfo_matches_stock_git_for_common_patch_mail`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `mailinfo` was already
  represented and these rows split exact positional output path surfaces
- Rust behavior changes: no

Expected rows:

- `git mailinfo git-msg git-patch < patch-mail.txt` as `<positional:msg>`
- `git mailinfo git-msg git-patch < patch-mail.txt` as `<positional:patch>`

The evidence compares stock Git and Zmin stdout plus the resulting message and
patch output files for the default patch-mail split flow.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already represented in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_mail_tools_compat::request_pull_matches_stock_git_for_local_pushed_branch`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+0`; `request-pull` was already
  represented and these rows split exact positional input surfaces
- Rust behavior changes: no

Expected rows:

- `git request-pull <start> file://<remote> main` as `<positional:url>`
- `git request-pull <start> file://<remote> main` as `<positional:end>`

The evidence compares stock Git and Zmin output for a local bare remote with a
pushed branch ahead of the start commit.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows and `+0` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already represented in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_mail_tools_compat::mailsplit_matches_stock_git_for_mbox_and_maildir`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `mailsplit -f`; the
  positional mbox input row is not represented in the Git docs option seed
- Rust behavior changes: no

Expected rows:

- `git mailsplit -d4 -f3 -oout mbox` as `-f 3`
- `git mailsplit -d4 -f3 -oout mbox` as `<positional:paths> mbox`

The evidence compares stock Git and Zmin stdout plus output files for the mbox
split flow.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows and `+1` represented doc-option pairs.

## Latest Declared Import

Source bucket: focused stock-oracle test already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_ref_resolution_compat::replace_matches_stock_git_for_list_create_delete`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+1`
- represented doc-option pairs: expected `+3` for `replace -d`,
  `replace -l` and `replace --format`; default and positional creation rows
  are not represented in the Git docs option seed
- Rust behavior changes: no

Expected rows:

- `git replace <object> <replacement>`
- `git replace`
- `git replace -l '*'`
- `git replace --format=medium`
- `git replace --format=long`
- `git replace -d <object-prefix>`

The evidence compares stock Git and Zmin output plus replacement ref side
effects for create, list, formatted list and delete flows.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+1` command with rows and `+3` represented doc-option pairs.

## Latest Declared Import

Source bucket: census implemented-but-unverified `checkout-index` schema
surfaces, then focused stock-oracle evidence already listed in
`docs/cli/existing_oracle_test_inventory.tsv`.

Evidence function:

- `git_worktree_state_compat::checkout_index_matches_stock_git_for_all_paths_stdin_and_prefix`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+1`
- missing-or-unclassified oracle functions: `-1`
- commands with rows: `+0`
- represented doc-option pairs: expected `+3` for `checkout-index -a`,
  `--prefix` and `--stdin`; the explicit path row is not represented in the
  Git docs option seed
- implemented-but-unverified schema rows: expected `-5` because `-a` closes
  the shared `arg_id=all` parser surface for the `--all` alias
- Rust behavior changes: no

Expected rows:

- `git checkout-index -a`
- `git checkout-index README.md`
- `git checkout-index --prefix=out/ README.md docs/guide.md`
- `printf 'docs/guide.md\n' | git checkout-index --stdin`

The evidence compares stock Git and Zmin stdout, stderr and restored worktree
file contents for all-path, explicit-path, prefixed-output and stdin-path
flows.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+1`
represented oracle function, `-1` missing-or-unclassified oracle function,
`+0` commands with rows, `+3` represented doc-option pairs and `-5`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `checkout-index` quiet schema
surfaces, with new focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-checkout-index-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `checkout-index --quiet`
  and `checkout-index -q`
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: no

Expected rows:

- `git checkout-index --quiet README.md`
- `git checkout-index -q docs/guide.md`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
worktree status and restored file contents for quiet long and short forms.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `symbolic-ref` positional
schema surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-symbolic-ref-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; these rows close positional schema
  surfaces rather than documented option spellings
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: no

Expected rows:

- `git symbolic-ref HEAD`
- `git symbolic-ref HEAD refs/heads/plumbing`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`.git/HEAD` content and worktree status in an attached repository with an
existing target branch.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0`
commands with rows, `+0` represented doc-option pairs and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `ls-tree` positional schema
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-ls-tree-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; these rows close positional schema
  surfaces rather than documented option spellings
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: no

Expected rows:

- `git ls-tree HEAD`
- `git ls-tree HEAD src/main.rs`

The evidence compares stock Git and Zmin exit status, stdout, stderr and
worktree status in a repository with root and nested files.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0`
commands with rows, `+0` represented doc-option pairs and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `hash-object` schema
surfaces, with focused stock-oracle smoke evidence and one parser schema fix.

Evidence command:

- `tools/git-hash-object-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+1`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `hash-object -t`; `--type`
  is an invalid undocumented spelling and does not add a Git doc-option pair
- implemented-but-unverified schema rows: expected `-2`; `-t` gets exact
  stock-Git evidence and `--type` is removed from the schema after proving
  stock Git rejects it
- Rust behavior changes: yes, parser schema removes `hash-object --type` and
  the pre-clap guard returns stock-style exit `129`/stderr for that spelling

Expected rows:

- `git hash-object -t blob a.txt`
- `git hash-object --type blob a.txt`

The evidence compares stock Git and Zmin exit status, stdout, stderr and
worktree status in a local repository with a regular file.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+1` closed row, `+0` open rows, `+1` invalid-input row, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0`
commands with rows, `+1` represented doc-option pair and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `mktree --missing` schema
surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-mktree-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `mktree --missing`
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `printf '100644 blob <missing-oid>\tmissing.txt\n' | git mktree --missing`

The evidence compares stock Git and Zmin exit status, stdout, stderr and the
written tree object type using stock Git.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+1` represented doc-option pair and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `commit-tree`
`<positional:tree>` schema surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-commit-tree-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; this is a positional schema surface and
  is not represented as a separate Git doc option seed row by the current census
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `git commit-tree <tree> -m root`

The evidence compares stock Git and Zmin exit status, stdout, stderr and the
stored commit object using fixed identity and timestamps.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+0` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `read-tree`
`<positional:treeish>` schema surface, with focused stock-oracle smoke
evidence.

Evidence command:

- `tools/git-read-tree-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; this is a positional schema surface and
  is not represented as a separate Git doc option seed row by the current census
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `git read-tree <tree>`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`ls-files --stage`, `write-tree` output and worktree status.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+0` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `write-tree --missing-ok`
schema surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-write-tree-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `write-tree --missing-ok`
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `git write-tree --missing-ok` with an index entry whose blob object is absent

The evidence compares stock Git and Zmin exit status, stdout, stderr and the
written tree object type using stock Git.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+1` represented doc-option pair and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `credential-cache --timeout`
and positional action schema surfaces, with focused stock-oracle smoke
evidence.

Evidence command:

- `tools/git-credential-cache-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `credential-cache --timeout`
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: yes, remove the extra blank line from
  `credential-cache get` output

Expected rows:

- `git credential-cache --socket=<path> --timeout=60 store/get/erase`
- positional `store`, `get` and `erase` action surface with `--timeout=60`

The evidence compares stock Git and Zmin exit status, stdout and stderr for
store, get, erase and post-erase get over explicit Unix socket paths.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+1` represented doc-option pair and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `credential-store --file`
and positional action schema surfaces, with focused stock-oracle smoke
evidence.

Evidence command:

- `tools/git-credential-store-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+1` for `credential-store --file`
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: no

Expected rows:

- `git credential-store --file <path> store/get/erase`
- positional `store`, `get` and `erase` action surface with `--file <path>`

The evidence compares stock Git and Zmin exit status, stdout, stderr, explicit
credential file side effects and the absence of default HOME credential-file
side effects.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+1` represented doc-option pair and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `count-objects` long option
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-count-objects-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for
  `count-objects --verbose` and `count-objects --human-readable`; the combined
  row extends both already represented long option spellings
- implemented-but-unverified schema rows: `+0`; the current census keeps these
  schema args unchanged while the rows add exact long-option evidence
- Rust behavior changes: no

Expected rows:

- `git count-objects --verbose`
- `git count-objects --human-readable`
- `git count-objects --verbose --human-readable`

The evidence compares stock Git and Zmin exit status, stdout, stderr and
worktree status in a repository with loose objects.

Actual post-import movement matched the declaration: `+3` behavior rows,
`+3` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `+0`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `archive -l` and `archive -v`
doc-option surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-archive-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `archive -l` and
  `archive -v`
- implemented-but-unverified schema rows: `+0`; these rows close short
  documented alias evidence while the current census keeps schema args
  unchanged
- Rust behavior changes: no

Expected rows:

- `git archive -l`
- `git archive --format=tar -v --output=out.tar HEAD dir`

The evidence compares stock Git and Zmin exit status, stdout, stderr and
worktree status. The `-v` row also compares the tar entry listing for the
written output file. The same probe found `git archive --output=out.tar HEAD`
produces a binary tar difference, so `--output` remains unverified and is not
counted in this import.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `+0`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `add --verbose` and `add -v`
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-add-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `add --verbose` and
  `add -v`
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: yes, `add` now prints stock-compatible
  `add '<path>'` rows for successful normal verbose staging

Expected rows:

- `git add --verbose verbose.txt`
- `git add -v verbose.txt`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`status --short` and stable `ls-files --stage --debug` index fields.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `add` schema/doc-option
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-add-oracle-smoke.sh`

Expected movement:

- behavior rows: `+5`
- closed rows: `+5`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+5` for `add --all`,
  `add --force`, `add --pathspec-file-nul`, `add --update` and `add -n`
- implemented-but-unverified schema rows: expected `-1`; the census maps these
  added rows onto one remaining schema gap, while the other option spellings
  become represented doc-option evidence and still require broader expansion
- Rust behavior changes: no

Expected rows:

- `git add --all`
- `git add --force force.ignored`
- `git add --pathspec-from-file=paths.nul --pathspec-file-nul`
- `git add --update`
- `git add -n dry.txt`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`status --short` and stable `ls-files --stage --debug` index fields. The same
probe found that `git add --verbose verbose.txt` and `git add -v verbose.txt`
still differ because stock Git prints `add 'verbose.txt'` while Zmin is silent;
those verbose surfaces remain implemented-but-unverified and are not counted in
this import.

Actual post-import movement matched the declaration: `+5` behavior rows,
`+5` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+5` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `commit` short/options
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-commit-short-options-oracle-smoke.sh`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+6` for `commit -q`, `commit -s`,
  `commit -o`, `commit -n`, `commit -t` and `commit --verbose`
- implemented-but-unverified schema rows: `+0`; these are doc-option evidence
  gaps after related schema arg IDs were already covered or remain schema
  aliases rather than new command entries
- Rust behavior changes: no

Expected rows:

- `git -c commit.gpgsign=false commit -q -m quiet`
- `git -c commit.gpgsign=false commit -s -m subject`
- `git -c commit.gpgsign=false commit -o -m only -- a.txt`
- `git -c commit.gpgsign=false commit -n -m skip`
- `GIT_EDITOR=.git/editor.sh git -c commit.gpgsign=false commit -t template.txt`
- `GIT_EDITOR=.git/editor.sh git -c commit.gpgsign=false commit --verbose --no-status`

The evidence compares stock Git and Zmin exit status, stdout, stderr, commit
object, HEAD and porcelain status. The `-n` row also verifies that a failing
pre-commit hook is skipped by comparing hook-log presence. The smoke disables
ambient commit signing with `-c commit.gpgsign=false` so local global Git config
cannot change the oracle commit object.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+6` represented doc-option pairs and `+0`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `commit` doc-option alias
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-commit-alias-oracle-smoke.sh`

Expected movement:

- behavior rows: `+4`
- closed rows: `+4`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+4` for `commit --file`,
  `commit --edit`, `commit --reuse-message` and `commit --reedit-message`
- implemented-but-unverified schema rows: `+0`; these are doc-option evidence
  gaps after the matching schema arg IDs were already covered by existing
  short-option rows
- Rust behavior changes: no

Expected rows:

- `git -c commit.gpgsign=false commit --file msg.txt`
- `GIT_EDITOR=.git/editor.sh git -c commit.gpgsign=false commit --edit --file msg.txt`
- `git -c commit.gpgsign=false commit --reuse-message HEAD~1`
- `GIT_EDITOR=.git/editor.sh git -c commit.gpgsign=false commit --reedit-message HEAD~1`

The evidence compares stock Git and Zmin exit status, stdout, stderr, commit
object, HEAD and porcelain status. The smoke disables ambient commit signing
with `-c commit.gpgsign=false` so local global Git config cannot change the
oracle commit object.

Actual post-import movement matched the declaration: `+4` behavior rows,
`+4` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+4` represented doc-option pairs and `+0`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `add` schema surfaces, with
new focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-add-oracle-smoke.sh`

Expected movement:

- behavior rows: `+3`
- closed rows: `+3`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `add --intent-to-add` and
  `add -N`; the positional path row is not represented in the Git docs option
  seed
- implemented-but-unverified schema rows: expected `-3`
- Rust behavior changes: no

Expected rows:

- `git add --intent-to-add intent.txt`
- `git add -N intent.txt`
- `git add new.txt`

The evidence compares stock Git and Zmin exit status, stdout, stderr,
`status --short` and stable `ls-files --stage --debug` index fields for
intent-to-add and normal positional add flows. The same smoke initially found
that `git add --verbose new.txt` prints `add 'new.txt'` while Zmin is silent;
that surface is not counted as verified in this import.

Actual post-import movement matched the declaration: `+3` behavior rows,
`+3` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `-3`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `bugreport` schema surfaces,
with new focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-bugreport-oracle-smoke.sh`

Expected movement:

- behavior rows: `+6`
- closed rows: `+6`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+4` for `bugreport --no-suffix`,
  `bugreport -o`, `bugreport --output-directory` and `bugreport -s`; the
  extra `--suffix` value-form rows extend an already represented option
- implemented-but-unverified schema rows: expected `-1` because
  `--no-suffix` closes a schema arg; `-o`, `--output-directory`, `-s` and
  `--suffix` are documented option rows, but the matching schema arg IDs were
  already covered by existing suffix evidence or remain doc-expansion work
- Rust behavior changes: no

Expected rows:

- `git bugreport -o <out> --no-suffix`
- `git bugreport --output-directory <out> --no-suffix`
- `git bugreport -o <out> -s custom`
- `git bugreport -o <out> --suffix=eqcustom`
- `git bugreport -o <out> --suffix sepcustom`
- an explicit `-o` row using the no-suffix evidence

The evidence compares stock Git and Zmin exit status, stdout, stderr and
created report filenames. It does not count `bugreport --diagnose=stats`
because that probe emits different diagnostic payload, so `--diagnose` remains
implemented-but-unverified.

Actual post-import movement matched the declaration: `+6` behavior rows,
`+6` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+4` represented doc-option pairs and `-1`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `cat-file --no-filter`
schema surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-cat-file-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; this is a Zmin schema surface and is
  not represented as a separate Git doc option seed row by the current census
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `printf '<oid>\n' | git cat-file --batch-check --no-filter`

The evidence compares stock Git and Zmin exit status, stdout, stderr and clean
worktree status in a repository with a committed blob.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+0` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `commit --all` schema
surfaces, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-commit-all-oracle-smoke.sh`

Expected movement:

- behavior rows: `+2`
- closed rows: `+2`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: expected `+2` for `commit --all` and
  `commit -a`
- implemented-but-unverified schema rows: expected `-2`
- Rust behavior changes: no

Expected rows:

- `git -c commit.gpgsign=false commit --all -m all-long`
- `git -c commit.gpgsign=false commit -a -m all-short`

The evidence compares stock Git and Zmin exit status, stdout, stderr, commit
object, HEAD, tree and porcelain status for tracked modifications with an
unrelated untracked file. The smoke disables ambient commit signing with
`-c commit.gpgsign=false` so local global Git config cannot change the oracle
commit object.

Actual post-import movement matched the declaration: `+2` behavior rows,
`+2` closed rows, `+0` open rows, `+0` invalid-input rows, `+0`
represented oracle functions, `+0` missing-or-unclassified oracle functions,
`+0` commands with rows, `+2` represented doc-option pairs and `-2`
implemented-but-unverified schema rows.

## Latest Declared Import

Source bucket: census implemented-but-unverified `merge-base` positional
schema surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-merge-base-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; this row closes a positional schema
  surface rather than a documented option spelling
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `git merge-base main side`

The evidence compares stock Git and Zmin exit status, stdout, stderr and
worktree status using copied workdirs from one seed repository so object ids
match byte-for-byte.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0`
commands with rows, `+0` represented doc-option pairs and `-1`
implemented-but-unverified schema row.

## Latest Declared Import

Source bucket: census implemented-but-unverified `show-ref` positional schema
surface, with focused stock-oracle smoke evidence.

Evidence command:

- `tools/git-show-ref-schema-oracle-smoke.sh`

Expected movement:

- behavior rows: `+1`
- closed rows: `+1`
- open rows: `+0`
- invalid-input rows: `+0`
- represented oracle functions: `+0`
- missing-or-unclassified oracle functions: `+0`
- commands with rows: `+0`
- represented doc-option pairs: `+0`; this row closes a positional schema
  surface rather than a documented option spelling
- implemented-but-unverified schema rows: expected `-1`
- Rust behavior changes: no

Expected row:

- `git show-ref refs/heads/main`

The evidence compares stock Git and Zmin exit status, stdout, stderr, full
`show-ref` output and worktree status using copied workdirs from one seed
repository so ref object ids match byte-for-byte.

Actual post-import movement matched the declaration: `+1` behavior row,
`+1` closed row, `+0` open rows, `+0` invalid-input rows, `+0` represented
oracle functions, `+0` missing-or-unclassified oracle functions, `+0`
commands with rows, `+0` represented doc-option pairs and `-1`
implemented-but-unverified schema row.
