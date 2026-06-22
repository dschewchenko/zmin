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
| Written behavior rows | `1094` | `2298` | `+1204` |
| Matching stock Git rows | `823` | `1976` | `+1153` |
| Open rows | `1` | `1` | `0` |
| Invalid-input rows | `270` | `321` | `+51` |
| Commands with rows | `50/151` | `96/151` | `+46` |
| Represented doc-option pairs | `253/4632` | `550/4632` | `+297` |

The text-level row delta audit reports `179` commits with `1293` TSV row
additions and `43` TSV row deletions, for `+1250` text net. The strict behavior
row count is `+1204` because some commits rewrote or split existing rows rather
than adding net-new row coverage.

The stock-oracle test inventory currently has `961` focused oracle functions:
`461` represented by matrix, extension or deferral evidence, and `500` still
missing or unclassified.

## Net Growth By Command

This table compares actual behavior rows per command at `9275ac4d` and at
`HEAD`.

| Command | Rows at `9275ac4d` | Rows at `HEAD` | Delta |
| --- | ---: | ---: | ---: |
| `diff` | `68` | `239` | `+171` |
| `stash` | `25` | `180` | `+155` |
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
| `log` | `87` | `104` | `+17` |
| `send-email` | `0` | `16` | `+16` |
| `interpret-trailers` | `0` | `15` | `+15` |
| `diff-index` | `0` | `14` | `+14` |
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
  functions, currently `961` total with `500` missing or unclassified.
- `docs/cli/git_compatibility_inventory.md`: command and documented option
  seed accounting, currently `151` commands and `4632` documented
  command-option pairs.
- `docs/cli/matrices/*_v2_47.tsv`: current written behavior rows.

These are not a complete final Git behavior denominator. They are the frozen
known inventory layers that must be expanded deliberately.
