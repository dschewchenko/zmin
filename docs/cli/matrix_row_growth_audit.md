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
| Written behavior rows | `1094` | `2354` | `+1260` |
| Matching stock Git rows | `823` | `2012` | `+1189` |
| Open rows | `1` | `1` | `0` |
| Invalid-input rows | `270` | `341` | `+71` |
| Commands with rows | `50/151` | `97/151` | `+47` |
| Represented doc-option pairs | `253/4632` | `550/4632` | `+297` |

The text-level row delta audit reports `190` commits with `1350` TSV row
additions and `43` TSV row deletions, for `+1307` text net. The strict behavior
row count is `+1260` because some commits rewrote or split existing rows rather
than adding net-new row coverage.

The stock-oracle test inventory currently has `961` focused oracle functions:
`500` represented by matrix, extension or deferral evidence, and `461` still
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
| `fsck` | `0` | `15` | `+15` |
| `cherry` | `0` | `6` | `+6` |
| `check-mailmap` | `0` | `6` | `+6` |
| `stripspace` | `0` | `5` | `+5` |
| `check-attr` | `0` | `5` | `+5` |
| `fetch` | `300` | `304` | `+4` |
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
| `add` | `3` | `6` | `+3` |
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
| `rm` | `0` | `1` | `+1` |
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
  functions, currently `961` total with `461` missing or unclassified.
- `docs/cli/git_compatibility_inventory.md`: command and documented option
  seed accounting, currently `151` commands and `4632` documented
  command-option pairs.
- `docs/cli/matrices/*_v2_47.tsv`: current written behavior rows.

These are not a complete final Git behavior denominator. They are the frozen
known inventory layers that must be expanded deliberately.

## Frozen Oracle Backlog Snapshot

This snapshot explains the remaining known denominator growth from focused
stock-oracle tests. It is intentionally file-level, not row-level: each listed
test function still must be read before adding TSV rows, because one function
can prove one row, several command variants, or a non-Git extension/deferral.

As of `5e182a2`, `docs/cli/existing_oracle_test_inventory.tsv` contains `961`
focused oracle functions. `500` are already represented by matrix rows,
extension rows or explicit deferrals, and `461` are
`missing_or_unclassified`.

Largest missing/unclassified buckets:

| Test file | Missing/unclassified functions |
| --- | ---: |
| `git_transport_http_compat.rs` | `75` |
| `git_transport_local_compat.rs` | `58` |
| `git_pack_integrity_compat.rs` | `46` |
| `git_index_mutation_compat.rs` | `39` |
| `git_maintenance_compat.rs` | `32` |
| `git_commit_compat.rs` | `26` |
| `git_worktree_state_compat.rs` | `26` |
| `git_notes_compat.rs` | `23` |
| `git_submodule_compat.rs` | `16` |
| `git_worktree_compat.rs` | `15` |
| `git_merge_compat.rs` | `13` |
| `git_sequencer_compat.rs` | `12` |
| `git_admin_tools_compat.rs` | `10` |
| `git_merge_plumbing_compat.rs` | `9` |
| `git_foreign_scm_compat.rs` | `8` |
| `git_global_cli_compat.rs` | `7` |
| `git_refs_compat.rs` | `7` |
| `git_ref_resolution_compat.rs` | `6` |
| `git_scalar_compat.rs` | `6` |
| `git_fast_import_export_compat.rs` | `5` |

Largest command-hint buckets inside those `461` functions:

| Command hint | Missing/unclassified functions |
| --- | ---: |
| `<none>` | `115` |
| `remote` | `58` |
| `worktree` | `48` |
| `commit` | `44` |
| `maintenance` | `34` |
| `config` | `33` |
| `refs` | `30` |
| `merge` | `29` |
| `notes` | `23` |
| `branch` | `20` |
| `submodule` | `18` |
| `add` | `17` |
| `upload-pack` | `14` |
| `fetch` | `12` |
| `prune` | `12` |
| `rebase` | `11` |
| `daemon` | `9` |
| `checkout` | `7` |
| `clean` | `5` |
| `clone` | `5` |
| `fast-import` | `5` |
| `status` | `5` |
| `tag` | `5` |

Use this snapshot as the upper bound for already-known oracle-import growth.
Future slices should reduce the `missing_or_unclassified` count by their
declared evidence-function count. If a slice increases written TSV rows without
reducing this count or without naming a different source bucket, that is a
process error.

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

## Latest Declared Import

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
