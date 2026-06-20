# Git Compatibility Inventory

This is the source of truth for counting Git compatibility work.

Command presence is not enough. Parser presence is not enough. A closed item is
a behavior variant checked against stock Git.

## Baseline

- Git baseline: `v2.47.1`
- Command source: upstream `command-list.txt`
- Option source: upstream `Documentation/git-*.txt`
- Behavior source: stock Git output, exit code and repository state
- Test source: local parity tests plus selected upstream Git tests

## Unit

One variant is:

`command + option + value + option combination + repository state + transport + platform`

Examples:

- `status -z --porcelain=v1` in a dirty worktree
- `status -z --porcelain=v2 --branch` in a repo with upstream tracking
- `fetch --depth=1 <remote> <refspec>` over smart HTTP
- `fetch --depth=1 <remote> <refspec> <refspec>` over smart HTTP
- `blame --date=relative -L 1,3 <path>`

These are separate rows because stock Git can produce different output,
different exit codes or different repository state.

## Files

- `tools/git-command-gap.sh` checks command entry points only.
- `tools/git-compat-option-inventory.sh` extracts a seed option list from Git
  `v2.47.1` documentation.
- `tools/git-compat-audit-summary.sh` combines command groups, option seed
  rows, command matrices and closed behavior blocks into the summary used by
  the README.
- `docs/cli/git_reference_groups.tsv` maps commands into git-scm reference
  groups. Commands can appear in more than one group.
- `docs/cli/git_audit_primary_groups.tsv` resolves duplicate group membership
  for closed behavior block reporting.
- `docs/cli/variant_compatibility_plan.md` tracks closed behavior blocks and
  open hard-fail clusters.
- `docs/cli/matrices/status_v2_47.tsv` is the first command-level matrix for
  Git `status`.

## Current Seed

The first documentation seed run on 2026-06-19 found:

- `2500` command-option rows
- `143` commands with extracted option rows
- `2500` unique command-option pairs

This is not the final denominator. It does not yet split option values,
negations, repeated options, order-sensitive combinations, repository states,
transports or platforms.
The seed extractor is intentionally conservative and can miss documented forms
that are hard to parse mechanically from prose. Command matrices may therefore
contain rows, such as `fetch --depth`, before the seed extractor learns that
spelling.

## Generated Summary

Run:

```bash
tools/git-compat-audit-summary.sh
```

Current generated summary:

| Git reference group | Git commands | Git doc option seed rows | Matrix rows | Matrix closed | Matrix partial | Matrix open | Matrix invalid input | Closed block variants |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Setup and Config | `6` | `76` | `0` | `0` | `0` | `0` | `0` | `0` |
| Getting and Creating Projects | `2` | `57` | `0` | `0` | `0` | `0` | `0` | `2` |
| Basic Snapshotting | `9` | `252` | `60` | `56` | `0` | `0` | `4` | `64` |
| Branching and Merging | `9` | `267` | `0` | `0` | `0` | `0` | `0` | `30` |
| Sharing and Updating Projects | `5` | `119` | `44` | `31` | `0` | `13` | `0` | `0` |
| Inspection and Comparison | `7` | `108` | `0` | `0` | `0` | `0` | `0` | `8` |
| Patching | `5` | `179` | `0` | `0` | `0` | `0` | `0` | `0` |
| Debugging | `3` | `127` | `0` | `0` | `0` | `0` | `0` | `52` |
| Email | `6` | `229` | `0` | `0` | `0` | `0` | `0` | `0` |
| External Systems | `2` | `95` | `0` | `0` | `0` | `0` | `0` | `0` |
| Administration | `8` | `134` | `0` | `0` | `0` | `0` | `0` | `17` |
| Server Admin | `2` | `28` | `0` | `0` | `0` | `0` | `0` | `0` |
| Plumbing Commands | `20` | `332` | `0` | `0` | `0` | `0` | `0` | `76` |
| Other Git 2.47 commands | `71` | `600` | `0` | `0` | `0` | `0` | `0` | `4` |
| **Git 2.47 unique total** | **`151`** | **`2500`** | **`104`** | **`87`** | **`0`** | **`13`** | **`4`** | **`253`** |

The matrix columns are command-level matrices with explicit option/value/state
rows. Closed block variants are focused parity blocks from
`docs/cli/variant_compatibility_plan.md`; they are not a full denominator.
Reference group rows follow git-scm sections and can duplicate command names.
The total row is unique.

## Command Matrices

| Command | Matrix | Total rows | Closed | Partial | Open | Invalid input |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `status` | `docs/cli/matrices/status_v2_47.tsv` | `60` | `56` | `0` | `0` | `4` |
| `fetch` | `docs/cli/matrices/fetch_v2_47.tsv` | `44` | `31` | `0` | `13` | `0` |

Selected closed behavior blocks without a full command matrix yet. The full
closed block list is in `docs/cli/variant_compatibility_plan.md` and is counted
by `tools/git-compat-audit-summary.sh`.

| Command | Closed variants | Evidence |
| --- | ---: | --- |
| `for-each-ref` date atoms | `16` | `git_for_each_ref_compat::for_each_ref_date_atoms_match_stock_git` |
| `for-each-ref` author atoms | `10` | `git_for_each_ref_compat::for_each_ref_date_atoms_match_stock_git` |
| `for-each-ref` tagger identity atoms | `2` | `git_for_each_ref_compat::for_each_ref_date_atoms_match_stock_git` |
| `for-each-ref` committer identity atoms | `2` | `git_for_each_ref_compat::for_each_ref_date_atoms_match_stock_git` |
| `for-each-ref` object size atom | `4` | `git_for_each_ref_compat::for_each_ref_matches_stock_git_for_common_formats` |
| `for-each-ref` object size sort key | `1` | `git_for_each_ref_compat::for_each_ref_matches_stock_git_for_common_formats` |
| `for-each-ref` refname strip modifiers | `10` | `git_for_each_ref_compat::for_each_ref_matches_stock_git_for_common_formats` |
| `for-each-ref` creator atoms | `18` | `git_for_each_ref_compat::for_each_ref_date_atoms_match_stock_git` |
| `for-each-ref` object id abbreviation lengths | `3` | `git_for_each_ref_compat::for_each_ref_matches_stock_git_for_common_formats` |
| `for-each-ref` invalid object id abbreviation lengths | `4` | `git_for_each_ref_compat::for_each_ref_objectname_short_invalid_lengths_match_stock_git` |
| `for-each-ref` invalid refname strip values | `6` | `git_for_each_ref_compat::for_each_ref_refname_strip_invalid_values_match_stock_git` |

The `status` matrix includes one newly closed row from this audit slice:
`git status -z` now matches stock Git's implicit porcelain v1 output. It also
promotes five parser-supported rows to closed evidence: `--null`, `--short`,
`-unormal`, bare `--untracked-files`, and `--ignored=traditional`. Existing
closed rows in that matrix are evidence import from current parity tests, not a
new support claim. The next slices closed `--ahead-behind` and
`--no-ahead-behind` for porcelain v1/v2 branch output with equal and different
upstream refs, then `--show-stash` and `--no-show-stash` for human output,
porcelain v2 output and order-sensitive toggle forms. The latest slice closed
`--long` and `--no-long`, including their order-sensitive interaction with
`--short`.
The following slice closed `--verbose` and `--no-verbose`, including `-v`,
`-vv`, order-sensitive reset forms and machine-readable combinations.
The latest slice closed `--column` and `--no-column` for human untracked
layout, order-sensitive reset forms, `--column=always/never`, `column.status`
and machine-readable combinations.
The latest audit reclassified `--untracked-cache`, `--no-untracked-cache`,
`--split-index`, and `--no-split-index` from open status gaps to
`invalid-input`: stock Git `2.47.1` rejects them for `git status` with exit
code `129`. They belong to `update-index`, not the `status` support surface.
The latest audit closed global `--no-optional-locks` for `status --short`
using existing global CLI parity evidence.
The latest implementation slice closed exact staged rename output for
`--renames`, `--no-renames`, `--find-renames`, and `--find-renames=<n>` across
human, porcelain v1/v2, and short forms.
The latest implementation slice closed `--ignore-submodules`, `=all`, `=dirty`
and `=untracked` for dirty, untracked and new-commit submodule states across
human, porcelain v1/v2 and short output.
The latest evidence slice closed standalone human `-b` and `--branch` status
for dirty, untracked and upstream-ahead states. The latest implementation slice
split the previous partial pathspec row into exact status-specific rows for
file, directory, default glob, explicit magic, exclude magic, human output and
global pathspec flags.

## Required Matrix Columns

The next compatibility matrix must use these columns:

| Column | Meaning |
| --- | --- |
| `group` | Git reference group |
| `command` | Git command name |
| `option` | option spelling or positional mode |
| `value` | accepted value, value class or empty |
| `combination` | required companion/conflicting options |
| `repo_state` | clean, dirty, conflicted, bare, submodule, worktree, shallow and so on |
| `transport` | local, file, smart HTTP, SSH, git daemon or empty |
| `platform` | all, macOS, Linux, Windows |
| `stock_git_case` | exact command used to produce expected behavior |
| `zmin_status` | `closed`, `open`, `partial`, `out-of-scope`, `invalid-input` |
| `evidence` | test name, upstream t-suite case or dogfood trace |

## Counting Rule

Only `closed` rows count as supported. `partial` rows do not count. Parser-only
rows do not count. `out-of-scope` rows must explain why they are not part of the
Git-compatible surface.

Do not publish a global percentage until every Git `v2.47.1` command has an
option/value matrix with statuses.
