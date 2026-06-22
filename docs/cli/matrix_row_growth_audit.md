# Matrix Row Growth Audit

This file explains behavior-row count growth on
`compat/status-pathspec-matrix` and defines the guardrail for future matrix
imports.

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

## Current Baseline

Pushed branch state audited from `9275ac4d` to `HEAD`:

| Metric | At `9275ac4d` | At `HEAD` | Delta |
| --- | ---: | ---: | ---: |
| Written behavior rows | `1094` | `2539` | `+1445` |
| Matching stock Git rows | `823` | `2165` | `+1342` |
| Open rows | `1` | `1` | `0` |
| Invalid-input rows | `270` | `373` | `+103` |
| Commands with rows | `50/151` | `98/151` | `+48` |
| Represented doc-option pairs | `253/4632` | `589/4632` | `+336` |

The text-level row delta audit reports `223` commits with `1536` TSV row
additions and `43` TSV row deletions, for `+1493` text net. The strict behavior
row count is `+1445` because some commits rewrote or split existing rows rather
than adding net-new row coverage.

The stock-oracle test inventory currently has `961` focused oracle functions:
`601` represented by matrix, extension or deferral evidence, and `360` still
missing or unclassified.

## Net Growth By Command

This table compares actual behavior rows per command at `9275ac4d` and at
`HEAD`.

| Command | Rows at `9275ac4d` | Rows at `HEAD` | Delta |
| --- | ---: | ---: | ---: |
| `diff` | `68` | `239` | `+171` |
| `stash` | `25` | `207` | `+182` |
| `ls-files` | `72` | `155` | `+83` |
| `diff-tree` | `0` | `74` | `+74` |
| `blame` | `101` | `171` | `+70` |
| `config` | `60` | `123` | `+63` |
| `status` | `76` | `135` | `+59` |
| `clone` | `6` | `59` | `+53` |
| `show` | `0` | `34` | `+34` |
| `remote` | `0` | `32` | `+32` |
| `rev-list` | `0` | `28` | `+28` |
| `cat-file` | `8` | `33` | `+25` |
| `rev-parse` | `52` | `73` | `+21` |
| `diff-files` | `0` | `20` | `+20` |
| `apply` | `0` | `20` | `+20` |
| `check-ignore` | `0` | `19` | `+19` |
| `clean` | `12` | `30` | `+18` |
| `log` | `87` | `105` | `+18` |
| `send-email` | `0` | `16` | `+16` |
| `notes` | `0` | `42` | `+42` |
| `interpret-trailers` | `0` | `15` | `+15` |
| `diff-index` | `0` | `14` | `+14` |
| `reflog` | `2` | `15` | `+13` |
| `filter-branch` | `2` | `14` | `+12` |
| `var` | `0` | `12` | `+12` |
| `grep` | `0` | `12` | `+12` |
| `describe` | `0` | `12` | `+12` |
| `archive` | `1` | `12` | `+11` |
| `check-ref-format` | `0` | `11` | `+11` |
| `count-objects` | `0` | `8` | `+8` |
| `shortlog` | `0` | `6` | `+6` |
| `patch-id` | `0` | `6` | `+6` |
| `format-patch` | `0` | `6` | `+6` |
| `fsck` | `0` | `35` | `+35` |
| `commit` | `0` | `42` | `+42` |
| `cherry` | `0` | `6` | `+6` |
| `check-mailmap` | `0` | `6` | `+6` |
| `stripspace` | `0` | `5` | `+5` |
| `check-attr` | `0` | `5` | `+5` |
| `fetch` | `300` | `304` | `+4` |
| `ls-remote` | `2` | `23` | `+21` |
| `replay` | `0` | `4` | `+4` |
| `read-tree` | `0` | `4` | `+4` |
| `mailinfo` | `0` | `4` | `+4` |
| `hash-object` | `0` | `4` | `+4` |
| `for-each-repo` | `0` | `4` | `+4` |
| `fmt-merge-msg` | `0` | `4` | `+4` |
| `difftool` | `0` | `4` | `+4` |
| `credential-cache` | `0` | `4` | `+4` |
| `credential` | `0` | `4` | `+4` |
| `commit-tree` | `0` | `4` | `+4` |
| `add` | `3` | `39` | `+36` |
| `unpack-file` | `0` | `3` | `+3` |
| `range-diff` | `0` | `3` | `+3` |
| `mktree` | `0` | `3` | `+3` |
| `credential-store` | `0` | `3` | `+3` |
| `bugreport` | `0` | `3` | `+3` |
| `write-tree` | `0` | `2` | `+2` |
| `update-server-info` | `0` | `2` | `+2` |
| `quiltimport` | `0` | `2` | `+2` |
| `mktag` | `0` | `2` | `+2` |
| `mailsplit` | `0` | `2` | `+2` |
| `get-tar-commit-id` | `0` | `2` | `+2` |
| `show-ref` | `10` | `11` | `+1` |
| `show-index` | `1` | `2` | `+1` |
| `rm` | `0` | `10` | `+10` |
| `request-pull` | `0` | `1` | `+1` |
| `am` | `0` | `1` | `+1` |

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
  functions, currently `961` total with `360` missing or unclassified.
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
awk -F '\t' 'NR>1 { total++; c[$7]++ } END { printf "total=%d represented=%d missing_or_unclassified=%d\n", total, c["represented"], c["missing_or_unclassified"] }' docs/cli/existing_oracle_test_inventory.tsv
```

Then select rows only from `missing_or_unclassified`, read the exact test
function, declare the expected row delta below and rerun the same inventory
check after the slice. The expected invariant for an import from this backlog
is:

- `represented` increases by the declared evidence-function count.
- `missing_or_unclassified` decreases by the same count.
- `behavior_rows_written` grows only by the declared row count.
- Any different movement is a process error to investigate before committing.

## Frozen Oracle Backlog Snapshot

This snapshot explains the remaining known denominator growth from focused
stock-oracle tests. It is intentionally file-level, not row-level: each listed
test function still must be read before adding TSV rows, because one function
can prove one row, several command variants, or a non-Git extension/deferral.

As of this commit, `docs/cli/existing_oracle_test_inventory.tsv` contains `961`
focused oracle functions. `601` are already represented by matrix rows,
extension rows or explicit deferrals, and `360` are
`missing_or_unclassified`.

Largest missing/unclassified buckets:

| Test file | Missing/unclassified functions |
| --- | ---: |
| `git_transport_http_compat.rs` | `62` |
| `git_transport_local_compat.rs` | `58` |
| `git_maintenance_compat.rs` | `32` |
| `git_pack_integrity_compat.rs` | `28` |
| `git_worktree_state_compat.rs` | `26` |
| `git_submodule_compat.rs` | `16` |
| `git_worktree_compat.rs` | `15` |
| `git_notes_compat.rs` | `14` |
| `git_merge_compat.rs` | `13` |
| `git_sequencer_compat.rs` | `12` |
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

Largest command-hint buckets inside those `360` functions:

| Command hint | Missing/unclassified functions |
| --- | ---: |
| `<none>` | `86` |
| `remote` | `54` |
| `worktree` | `47` |
| `maintenance` | `34` |
| `merge` | `29` |
| `refs` | `22` |
| `branch` | `20` |
| `commit` | `18` |
| `submodule` | `17` |
| `notes` | `14` |
| `upload-pack` | `14` |
| `add` | `13` |
| `prune` | `12` |
| `config` | `11` |
| `rebase` | `11` |

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
| 1 | `git_transport_http_compat.rs` (`62`) | largest remaining source and likely dense `remote`/`upload-pack` transport rows |
| 2 | `git_transport_local_compat.rs` (`58`) | second-largest source with local/file transport and remote-management rows |
| 3 | `git_maintenance_compat.rs` (`32`) | dense maintenance/repack/multi-pack-index rows with shared command shape |
| 4 | `git_pack_integrity_compat.rs` (`28`) | pack/fsck/bundle rows; continue here only when the selected function group is coherent |
| 5 | `git_worktree_state_compat.rs` (`26`) | worktree state rows that may expose implementation gaps |

If a new WebStorm or replacement-binary blocker appears, it overrides this
walk order. If a selected bucket produces Zmin-only extension behavior or an
intentional deferral instead of Git matrix rows, record that classification and
do not increase written behavior rows.

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
