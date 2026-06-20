# Git Compatibility Inventory

This is the source of truth for counting Git compatibility work.

Command presence is not enough. Parser presence is not enough. Test presence is
not enough. A closed item is one behavior variant checked against stock Git.
Everything else is inventory or audit progress.

## Baseline

- Git baseline: `v2.47.1`
- Command source: upstream `command-list.txt`
- Option source: upstream `Documentation/git-*.txt` plus included option files
- Behavior source: stock Git output, exit code and repository state
- Test source: local parity tests plus selected upstream Git tests

## Unit

One behavior variant is:

`command + option + value + option combination + repository state + transport + platform`

Examples:

- `status -z --porcelain=v1` in a dirty worktree
- `status -z --porcelain=v2 --branch` in a repo with upstream tracking
- `fetch --depth=1 <remote> <refspec>` over smart HTTP
- `fetch --depth=1 <remote> <refspec> <refspec>` over smart HTTP
- `blame --date=relative -L 1,3 <path>`

These are separate rows because stock Git can produce different output,
different exit codes or different repository state.

The option seed is not this denominator. A single documented spelling such as
`--date`, `--format`, `--pathspec-from-file` or `--depth` can expand into many
rows once values, repeated forms, option order, repository state, transport and
platform behavior are included.

Example expansion for one option:

| Seed option | Expansion examples |
| --- | --- |
| `status -z` | implicit porcelain v1, explicit porcelain v1, porcelain v2, with and without branch headers, clean/dirty/staged/untracked states |
| `blame --date` | documented date modes, invalid values, custom format values, locale/timezone effects where stock Git exposes them |
| `fetch --depth` | named remote, explicit path, `file://`, smart HTTP, SSH, git daemon, single refspec, multiple refspecs, branchless HEAD, shallow source, repeated options |

## Audit Workflow

Compatibility work must start from the inventory, not from the current parser.

1. List Git `v2.47.1` commands from upstream command sources.
2. Seed every documented option spelling from Git docs.
3. Expand those option spellings into values, negations, repeated forms,
   order-sensitive combinations, positional modes, repository states,
   transports and platforms.
4. Add upstream Git test cases and real tool traces, such as IDE command lines,
   when they expose behavior not obvious from docs.
5. Record the stock Git command line and expected output, exit code and
   repository state for each row.
6. Mark a row `closed` only when Zmin has focused parity evidence for that
   exact row.
7. Implement missing behavior only after the row is classified; do not count
   parser acceptance, command dispatch or a broad smoke test as support.
8. Add focused tests for each closed row. Prefer stock Git as the expected
   result, not hand-written expected output, when the behavior is observable.

Current matrices are still being expanded. A command with no open rows in the
current matrix is not automatically complete; it only has no open row among the
variants written down so far.

## Completion Rule

Do not call a command complete until all of these inputs have been reconciled:

- upstream Git command list for `v2.47.1`
- documented options from `Documentation/git-*.txt` and included option files
- option values, missing-value defaults, negations and repeated forms
- order-sensitive option combinations, including last-option-wins cases
- positional forms, pathspec magic and stdin/file-list modes
- repository states: clean, dirty, staged, conflicted, bare, shallow,
  submodule, linked worktree, unborn branch and detached `HEAD`
- transports: local path, `file://`, smart HTTP, SSH, git daemon and bundles
- platform behavior on macOS, Linux and Windows
- selected upstream Git test cases
- real tool traces from IDEs and GUI clients

Each row must store the stock Git command line, exit code, stdout/stderr shape
and repository-state expectations. Zmin support for that row is closed only when
focused parity evidence checks the same surface.

## Files

- `tools/git-command-gap.sh` checks command entry points only.
- `tools/git-compat-option-inventory.sh` extracts a seed option list from Git
  `v2.47.1` documentation.
- `tools/git-compat-audit-summary.sh` combines command groups, option seed
  rows, command matrices and closed behavior blocks into the summary used by
  the README.
- `tools/git-compat-command-summary.sh` reports complete command matrices,
  commands with matrix rows, represented doc-option pairs and written behavior
  rows.
- `docs/cli/git_reference_groups.tsv` maps commands into git-scm reference
  groups. Commands can appear in more than one group.
- `docs/cli/git_audit_primary_groups.tsv` resolves duplicate group membership
  for closed behavior block reporting.
- `docs/cli/variant_compatibility_plan.md` tracks closed behavior blocks and
  open hard-fail clusters.
- `docs/cli/matrices/status_v2_47.tsv` is the first command-level matrix for
  Git `status`.
- `docs/cli/matrices/fetch_v2_47.tsv` tracks the first `fetch` option,
  transport and repository-state variants.

## Current Seed

The current documentation seed run found:

- `4632` command-option rows
- `143` commands with extracted option rows
- `4632` unique command-option pairs

This is not the final denominator. It does not yet split option values,
negations, repeated options, order-sensitive combinations, repository states,
transports or platforms. It is only the raw input used to build command
matrices.
The seed extractor is intentionally conservative and can miss documented forms
that are hard to parse mechanically from prose. Command matrices may therefore
contain rows, such as `fetch --depth`, before the seed extractor learns that
spelling.

## Denominator Layers

Do not collapse these layers into one percentage.

| Layer | Count | Counts as support | Meaning |
| --- | ---: | --- | --- |
| Fully complete command matrices | `0/151` | yes, when complete | no command matrix is complete yet |
| Commands with any matrix rows | `2/151` | no | audit rows exist only for `status` and `fetch` |
| Git doc option pairs represented by rows | `50/4632` | no | documented command-option pairs with at least one behavior row |
| Written behavior rows | `191` | no by itself | explicit command/option/value/combination/state/transport/platform rows currently written |
| Written rows matching stock Git | `176/191` | yes, row by row | exact written rows with parity evidence |
| Full Git behavior denominator | not known yet | not yet | still being expanded |

The full denominator must include command, option, value, option combination,
repository state, transport and platform. It also needs rows from Git docs,
upstream Git tests and real tool traces such as IDE or GUI invocations.

Unknown rows are not allowed to disappear from reporting. If a command matrix is
not fully expanded, the command remains incomplete even when every written row
is closed.

Until a command's matrix has all of those rows, that command remains
incomplete even if every currently written row is closed.

## Generated Summary

Run:

```bash
tools/git-compat-audit-summary.sh
tools/git-compat-command-summary.sh
```

Current generated summary:

| Git reference group | Git commands | Git doc option seed rows | Matrix rows | Written rows matching stock Git | Matrix partial | Matrix open | Matrix invalid input | Closed block variants |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Setup and Config | `6` | `276` | `0` | `0` | `0` | `0` | `0` | `0` |
| Getting and Creating Projects | `2` | `66` | `0` | `0` | `0` | `0` | `0` | `2` |
| Basic Snapshotting | `9` | `371` | `60` | `56` | `0` | `0` | `4` | `64` |
| Branching and Merging | `9` | `581` | `0` | `0` | `0` | `0` | `0` | `30` |
| Sharing and Updating Projects | `5` | `309` | `131` | `120` | `0` | `9` | `2` | `20` |
| Inspection and Comparison | `7` | `774` | `0` | `0` | `0` | `0` | `0` | `8` |
| Patching | `5` | `333` | `0` | `0` | `0` | `0` | `0` | `0` |
| Debugging | `3` | `132` | `0` | `0` | `0` | `0` | `0` | `52` |
| Email | `6` | `361` | `0` | `0` | `0` | `0` | `0` | `0` |
| External Systems | `2` | `120` | `0` | `0` | `0` | `0` | `0` | `0` |
| Administration | `8` | `147` | `0` | `0` | `0` | `0` | `0` | `17` |
| Server Admin | `2` | `30` | `0` | `0` | `0` | `0` | `0` | `0` |
| Plumbing Commands | `20` | `644` | `0` | `0` | `0` | `0` | `0` | `76` |
| Other Git 2.47 commands | `71` | `1075` | `0` | `0` | `0` | `0` | `0` | `4` |
| **Git 2.47 unique total** | **`151`** | **`4632`** | **`191`** | **`176`** | **`0`** | **`9`** | **`6`** | **`273`** |

The matrix columns are the written subset of explicit
option/value/combination/state/transport/platform rows. They are not the final
denominator until each command matrix has been expanded from docs, upstream
tests and real traces. Closed block variants are focused parity blocks from
`docs/cli/variant_compatibility_plan.md`; they are not a full denominator.
Reference group rows follow git-scm sections and can duplicate command names.
The total row is unique.

Never use `151/151` command presence, `4632` option spellings or `176/191`
passing written rows as a Git support percentage. The `176/191` number is audit
progress for rows already written down. It says nothing about the still
unexpanded rows. A command is complete only after its documented options,
values, negations, repeated forms, order-sensitive combinations, repository
states, transports and platforms have behavior rows with stock-Git evidence.

## Command Matrices

These counts are for written rows only. A command can show no open row and
still be incomplete if the matrix has not expanded all Git-documented
variants.

| Command | Git doc option seed | Doc spellings represented by rows | Matrix | Behavior rows written | Written rows matching stock Git | Partial | Open | Invalid input | Complete matrix |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- |
| `status` | `26` | `22` | `docs/cli/matrices/status_v2_47.tsv` | `60` | `56` | `0` | `0` | `4` | no |
| `fetch` | `73` | `28` | `docs/cli/matrices/fetch_v2_47.tsv` | `131` | `120` | `0` | `9` | `2` | no |

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

The latest `fetch` slice closed `--recurse-submodules`, `=yes`, `=on-demand`,
`=no`, and `--no-recurse-submodules` only for repositories without submodules.
The submodule-enabled `on-demand` behavior remains open until recursive fetch
updates changed submodule commits like stock Git.
The latest `fetch --server-option` slice closed equals and separate-value forms
for local path and file URL remotes, where stock Git accepts the option as a
no-op. Smart HTTP/SSH protocol-v2 passthrough remains open.
The latest `fetch --upload-pack` slices closed equals and separate-value forms
for named local path and file URL remotes, configured fetch, explicit branch
`FETCH_HEAD` modes, multiple explicit refspecs, local/file `--all`,
local/file `--multiple` acceptance, local/file explicit-branch `--depth=1`,
local/file explicit-branch `--deepen=1`, local/file `--unshallow` forms and
local/file explicit-branch `--shallow-since` and `--shallow-exclude` forms in
existing shallow repos.
The latest `fetch --update-shallow` slices closed named local path/file URL
remotes plus explicit local path/file URL branch fetches where the source
remote itself is shallow.
The latest `fetch --shallow-since` slice closed explicit local path/file URL
branch fetches for equals and separate-value forms.
The latest `fetch --shallow-exclude` slice closed explicit local path/file URL
branch fetches for equals and separate-value forms.
The latest `fetch --deepen` slice closed explicit local path/file URL branch
fetches for equals and separate-value forms.
The latest `fetch --unshallow` slices closed explicit local path/file URL HEAD
and branch fetches for existing shallow repos.
Zmin invokes the external upload-pack command where stock Git does for those
local/file forms and preserves stock Git's local/file `--all` and `--multiple`
behavior, where the custom upload-pack command is not invoked. SSH upload-pack
override for shallow fetch modes remains open.

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
