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
- `docs/cli/variant_compatibility_plan.md` tracks closed behavior blocks and
  open hard-fail clusters.

## Current Seed

The first documentation seed run on 2026-06-19 found:

- `2500` command-option rows
- `143` commands with extracted option rows
- `2500` unique command-option pairs

This is not the final denominator. It does not yet split option values,
negations, repeated options, order-sensitive combinations, repository states,
transports or platforms.

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
