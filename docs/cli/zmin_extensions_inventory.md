# Zmin Extension Inventory

This document is a human projection of the machine contract in
`tools/zmin-extensions-contract.tsv`. The TSV is authoritative; this Markdown
file is not a compatibility denominator.

The Git compatibility status remains `compatibility_claim=unverified`. This
separate inventory must not be read as a current-Git parity claim or added to
the Git denominator.

## Current counts

| Layer | Count | Meaning |
| --- | ---: | --- |
| Primary stable rows | `40` | Implemented Zmin-only command, subcommand, option and environment rows |
| Primary deferred rows | `0` | No primary row is deferred |
| Primary rows | `40` | Stable primary rows |
| Neutral relationship rows | `7` | API relationships, never extensions or exclusions |
| Tracked contract rows | `47` | 40 primary rows plus 7 relationship-neutral rows |

The 40 primary rows are grouped as follows:

- 10 command rows: `hooks`, `save`, `changes`, `publish`, `update`, `undo`,
  `timeline`, `recover`, `compatibility`, and `lfs`.
- 5 `hooks` subcommands: `init`, `add`, `list`, `remove`, `run`.
- 24 options: four `clone` options (`--worktree-first`, `--instant`,
  `--background-fetch`, `--demand-hydrate`); four `cat-file` options
  (`--type`, `--size`, `--exists`, `--pretty`); three `imap-send` options
  (`--folder`, `--list`, `-f`); one `credential-cache` option
  (`--daemon-internal`); three `instaweb` options (`--daemon-internal`,
  `--git-dir`, `--work-tree`); and nine hook/report options
  (`--ignore-missing`, `--to-stdin`, `--force`, `--staged-runner`, `--ext`,
  `--staged`, `--list`, `--dry-run`). The repeated `--ext` surface is a
  separate contract row for its distinct hook parent.
- One environment row: `ZMIN_GIT_HTTP_VERSION`.

The seven neutral relationship rows are `hooks -> init`, `hooks -> add`,
`hooks -> list`, `hooks -> remove`, `hooks -> run`, `repo -> info`, and
`repo -> structure`. They describe API relationships and do not make `repo`
or any related surface a Zmin-only exclusion.

## Current Git names that are not extensions

The frozen Git v2.55.0 contract explicitly keeps these current command names
out of the Zmin primary extension set:

`backfill`, `diff-pairs`, `format-rev`, `history`, `last-modified`, `repo`,
`url-parse`.

In particular, `history` with `reword` and `split` is current Git behavior, and
`repo` is current-Git relationship evidence. Stock Git's singular `git hook
run` is also distinct from Zmin's plural `zmin hooks` product surface.

## Evidence boundary

The current Git target is defined independently by
`tools/git-upstream-compat-contract.tsv` and
`docs/git/upstream_compatibility_baseline.md`: the frozen nondeprecated surface
has 1045 of 1046 top-level Git v2.55.0 shell tests, with only the whole-file
`t5323-pack-redundant.sh` exclusion. The five retained legacy/current groups
and deprecated assertions inside retained mixed tests are not extra exclusions.
Extension rows never inflate that denominator and do not establish Git parity.

All 40 primary rows are stable. Together with the seven relationship-neutral
rows, the contract therefore has 47 tracked contract rows. That number is
neither 47 Git APIs nor a claim of arbitrary Git LFS ecosystem parity.
`command.lfs` covers the implemented Git LFS v3.7.1-compatible command and
transport slice, with local and HTTP Batch download/upload evidence in the
machine contract.
Configured mTLS/client identity remains an explicit transport exclusion and is
rejected before network access; it is not an exclusion from the Git
denominator. Custom transfer adapters and untracked Git LFS commands likewise
remain outside the extension row.
