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

The important distinction is this:

- `151` command names are the command-entrypoint inventory.
- `4632` documented command-option pairs are a seed extracted from Git docs.
- The real argument denominator is larger than `4632` because every option
  must split into values, missing-value defaults, negations, repeated forms,
  option ordering, positional arguments, repository states, transports and
  platforms.
- Zmin support is counted only row by row after stock Git parity evidence.

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
- `docs/cli/matrices/bisect_v2_47.tsv` tracks the first `bisect`
  invalid-input variant.
- `docs/cli/matrices/blame_v2_47.tsv` tracks the first `blame`
  invalid-input variant.
- `docs/cli/matrices/cat_file_v2_47.tsv` tracks the first `cat-file`
  invalid-input variant.
- `docs/cli/matrices/commit_graph_v2_47.tsv` tracks the first
  `commit-graph verify` header validation variants.
- `docs/cli/matrices/config_v2_47.tsv` tracks the first `config` read, write,
  include, typed-value and invalid-input variants.
- `docs/cli/matrices/fast_import_v2_47.tsv` tracks the first `fast-import`
  date-format value variant.
- `docs/cli/matrices/status_v2_47.tsv` is the first command-level matrix for
  Git `status`.
- `docs/cli/matrices/fetch_v2_47.tsv` tracks the first `fetch` option,
  transport and repository-state variants.
- `docs/cli/matrices/branch_v2_47.tsv` tracks the first `branch`
  show-current, list, format, upstream, merged/contains and invalid-input
  variants.
- `docs/cli/matrices/bundle_v2_47.tsv` tracks the first `bundle create`
  version value and invalid-input variants.
- `docs/cli/matrices/diff_v2_47.tsv` tracks the first `diff` output format,
  patch, reverse, pickaxe, ordering, path, exit-code and binary-text variants.
- `docs/cli/matrices/log_v2_47.tsv` tracks the first `log` format, traversal,
  decoration, merge-diff, date, reflog and IDE-shaped NUL-output variants.
- `docs/cli/matrices/for_each_ref_v2_47.tsv` tracks the first
  `for-each-ref` format, sort, refname/objectname modifier, date, upstream and
  invalid-input variants.
- `docs/cli/matrices/ls_files_v2_47.tsv` tracks the first `ls-files` cached,
  stage, raw index mode, NUL, EOL, ignored/others, unmerged and submodule
  variants.
- `docs/cli/matrices/ls_tree_v2_47.tsv` tracks the first `ls-tree` default,
  recursive, show-tree and invalid-input variants.
- `docs/cli/matrices/merge_v2_47.tsv` tracks the first `merge` strategy
  invalid-input variant.
- `docs/cli/matrices/merge_base_v2_47.tsv` tracks the first `merge-base`
  plain, is-ancestor, commit-graph, octopus and invalid-input variants.
- `docs/cli/matrices/multi_pack_index_v2_47.tsv` tracks the first
  `multi-pack-index verify` header validation variants.
- `docs/cli/matrices/pack_objects_v2_47.tsv` tracks the first
  `pack-objects --index-version` value variants.
- `docs/cli/matrices/reflog_v2_47.tsv` tracks the first `reflog`
  invalid-input and shorthand-ref variants.
- `docs/cli/matrices/rerere_v2_47.tsv` tracks the first `rerere`
  invalid-operation variant.
- `docs/cli/matrices/rev_parse_v2_47.tsv` tracks the first `rev-parse`
  discovery, path-format, revision and invalid-input variants.
- `docs/cli/matrices/show_index_v2_47.tsv` tracks the first `show-index`
  stdin pack-index invalid-input variant.
- `docs/cli/matrices/show_ref_v2_47.tsv` tracks the first `show-ref`
  heads, head, hash, tags, verify and invalid-input variants.
- `docs/cli/matrices/sparse_checkout_v2_47.tsv` tracks the first
  `sparse-checkout` invalid-input subcommand variant.
- `docs/cli/matrices/symbolic_ref_v2_47.tsv` tracks the first
  `symbolic-ref` write, read, short, no-recurse, quiet and invalid-input
  variants.
- `docs/cli/matrices/tag_v2_47.tsv` tracks the first `tag` listing, annotated
  tag, filter, sort, format and invalid-input variants.
- `docs/cli/matrices/version_v2_47.tsv` tracks the first `version` default,
  build-options and invalid-option variants used by replacement-binary
  dogfooding.
- `docs/cli/matrices/worktree_v2_47.tsv` tracks the first `worktree`
  invalid-input subcommand variant.

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
| Fully complete command-option matrices | `0/4632` | yes, when complete | no documented option spelling has a complete behavior matrix yet |
| Commands with any matrix rows | `37/151` | no | audit rows exist for `archive`, `bisect`, `blame`, `branch`, `bundle`, `cat-file`, `clean`, `column`, `commit-graph`, `config`, `diff`, `fast-import`, `fetch`, `for-each-ref`, `index-pack`, `log`, `ls-files`, `ls-tree`, `merge`, `merge-base`, `multi-pack-index`, `notes`, `pack-objects`, `rebase`, `reflog`, `rerere`, `rev-parse`, `show-index`, `show-ref`, `sparse-checkout`, `status`, `submodule`, `symbolic-ref`, `tag`, `verify-pack`, `version` and `worktree` |
| Git doc option pairs represented by rows | `241/4632` | no | documented command-option pairs with at least one behavior row |
| Written behavior rows | `1005` | no by itself | explicit command/option/value/combination/state/transport/platform rows currently written |
| Written rows matching stock Git | `770/1005` | yes, row by row | supported-behavior rows with parity evidence |
| Partial written rows | `1/1005` | no | written rows with incomplete parity |
| Full Git behavior denominator | not known yet | not yet | still being expanded |

The `4632` option count is only the documented Git 2.47 seed. The full
denominator must expand every command into command, option, accepted value,
missing-value default, negation, repeated form, option combination, repository
state, transport and platform axes. It also needs rows from Git docs, upstream
Git tests and real tool traces such as IDE or GUI invocations.

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

| Git reference group | Git commands | Complete command matrices | Git doc option seed rows | Complete doc option pairs | Matrix rows | Written rows matching stock Git | Matrix partial | Matrix open | Matrix invalid input | Closed block variants |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Setup and Config | `6` | `0` | `276` | `0` | `60` | `50` | `0` | `0` | `10` | `8` |
| Getting and Creating Projects | `2` | `0` | `66` | `0` | `0` | `0` | `0` | `0` | `0` | `2` |
| Basic Snapshotting | `9` | `0` | `371` | `0` | `81` | `66` | `0` | `0` | `15` | `96` |
| Branching and Merging | `9` | `0` | `581` | `0` | `60` | `35` | `0` | `0` | `25` | `32` |
| Sharing and Updating Projects | `5` | `0` | `309` | `0` | `297` | `283` | `0` | `0` | `14` | `159` |
| Inspection and Comparison | `7` | `0` | `774` | `0` | `152` | `147` | `0` | `0` | `5` | `26` |
| Patching | `5` | `0` | `333` | `0` | `1` | `0` | `0` | `0` | `1` | `1` |
| Debugging | `3` | `0` | `132` | `0` | `102` | `21` | `0` | `0` | `81` | `154` |
| Email | `6` | `0` | `361` | `0` | `0` | `0` | `0` | `0` | `0` | `0` |
| External Systems | `2` | `0` | `120` | `0` | `0` | `0` | `0` | `0` | `0` | `0` |
| Administration | `8` | `0` | `147` | `0` | `28` | `10` | `0` | `0` | `18` | `34` |
| Server Admin | `2` | `0` | `30` | `0` | `0` | `0` | `0` | `0` | `0` | `0` |
| Plumbing Commands | `20` | `0` | `644` | `0` | `195` | `151` | `0` | `0` | `44` | `110` |
| Other Git 2.47 commands | `71` | `0` | `1075` | `0` | `29` | `7` | `1` | `0` | `21` | `28` |
| **Git 2.47 unique total** | **`151`** | **`0`** | **`4632`** | **`0`** | **`1005`** | **`770`** | **`1`** | **`0`** | **`234`** | **`650`** |

The matrix columns are the written subset of explicit
option/value/combination/state/transport/platform rows. They are not the final
denominator until each command matrix has been expanded from docs, upstream
tests and real traces. Closed block variants are focused parity blocks from
`docs/cli/variant_compatibility_plan.md`; they are not a full denominator.
Reference group rows follow git-scm sections and can duplicate command names.
The total row is unique.

Never use `151/151` command presence, `4632` option spellings, `241/4632`
represented option pairs or `770/1005` passing written rows as a Git support
percentage. The `770/1005` number is audit progress for supported rows already
written down; `1/1005` row is partial and `234/1005` additional rows are
stock-compatible invalid inputs. It
says nothing about the still unexpanded rows. A command or option pair is
complete only after its documented values, negations, repeated forms,
order-sensitive combinations, repository states, transports and platforms have
behavior rows with stock-Git evidence.

## Command Matrices

These counts are for written rows only. A command can show no open row and
still be incomplete if the matrix has not expanded all Git-documented
variants.

| Command | Git doc option seed | Complete doc option pairs | Doc spellings represented by rows | Matrix | Behavior rows written | Written rows matching stock Git | Partial | Open | Invalid input | Complete matrix |
| --- | ---: | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- |
| `archive` | `17` | `0` | `1` | `docs/cli/matrices/archive_v2_47.tsv` | `1` | `0` | `0` | `0` | `1` | no |
| `bisect` | `18` | `0` | `0` | `docs/cli/matrices/bisect_v2_47.tsv` | `1` | `0` | `0` | `0` | `1` | no |
| `blame` | `39` | `0` | `6` | `docs/cli/matrices/blame_v2_47.tsv` | `101` | `21` | `0` | `0` | `80` | no |
| `branch` | `51` | `0` | `13` | `docs/cli/matrices/branch_v2_47.tsv` | `31` | `18` | `0` | `0` | `13` | no |
| `bundle` | `15` | `0` | `1` | `docs/cli/matrices/bundle_v2_47.tsv` | `11` | `3` | `0` | `0` | `8` | no |
| `cat-file` | `21` | `0` | `1` | `docs/cli/matrices/cat_file_v2_47.tsv` | `1` | `0` | `0` | `0` | `1` | no |
| `clean` | `13` | `0` | `3` | `docs/cli/matrices/clean_v2_47.tsv` | `11` | `7` | `0` | `0` | `4` | no |
| `column` | `10` | `0` | `1` | `docs/cli/matrices/column_v2_47.tsv` | `1` | `0` | `0` | `0` | `1` | no |
| `commit-graph` | `18` | `0` | `0` | `docs/cli/matrices/commit_graph_v2_47.tsv` | `3` | `0` | `0` | `0` | `3` | no |
| `config` | `243` | `0` | `17` | `docs/cli/matrices/config_v2_47.tsv` | `60` | `50` | `0` | `0` | `10` | no |
| `status` | `26` | `0` | `22` | `docs/cli/matrices/status_v2_47.tsv` | `75` | `66` | `0` | `0` | `9` | no |
| `fetch` | `73` | `0` | `30` | `docs/cli/matrices/fetch_v2_47.tsv` | `290` | `283` | `0` | `0` | `7` | no |
| `diff` | `133` | `0` | `31` | `docs/cli/matrices/diff_v2_47.tsv` | `65` | `65` | `0` | `0` | `0` | no |
| `fast-import` | `25` | `0` | `1` | `docs/cli/matrices/fast_import_v2_47.tsv` | `4` | `0` | `1` | `0` | `3` | no |
| `log` | `282` | `0` | `32` | `docs/cli/matrices/log_v2_47.tsv` | `87` | `82` | `0` | `0` | `5` | no |
| `for-each-ref` | `22` | `0` | `2` | `docs/cli/matrices/for_each_ref_v2_47.tsv` | `34` | `23` | `0` | `0` | `11` | no |
| `index-pack` | `18` | `0` | `1` | `docs/cli/matrices/index_pack_v2_47.tsv` | `3` | `0` | `0` | `0` | `3` | no |
| `ls-files` | `42` | `0` | `27` | `docs/cli/matrices/ls_files_v2_47.tsv` | `72` | `55` | `0` | `0` | `17` | no |
| `ls-tree` | `15` | `0` | `2` | `docs/cli/matrices/ls_tree_v2_47.tsv` | `4` | `3` | `0` | `0` | `1` | no |
| `merge` | `69` | `0` | `1` | `docs/cli/matrices/merge_v2_47.tsv` | `1` | `0` | `0` | `0` | `1` | no |
| `merge-base` | `27` | `0` | `2` | `docs/cli/matrices/merge_base_v2_47.tsv` | `12` | `10` | `0` | `0` | `2` | no |
| `multi-pack-index` | `10` | `0` | `0` | `docs/cli/matrices/multi_pack_index_v2_47.tsv` | `4` | `1` | `0` | `0` | `3` | no |
| `notes` | `33` | `0` | `0` | `docs/cli/matrices/notes_v2_47.tsv` | `6` | `0` | `0` | `0` | `6` | no |
| `pack-objects` | `44` | `0` | `1` | `docs/cli/matrices/pack_objects_v2_47.tsv` | `10` | `4` | `0` | `0` | `6` | no |
| `rebase` | `103` | `0` | `1` | `docs/cli/matrices/rebase_v2_47.tsv` | `1` | `0` | `0` | `0` | `1` | no |
| `reflog` | `13` | `0` | `0` | `docs/cli/matrices/reflog_v2_47.tsv` | `2` | `0` | `0` | `0` | `2` | no |
| `rerere` | `7` | `0` | `0` | `docs/cli/matrices/rerere_v2_47.tsv` | `1` | `0` | `0` | `0` | `1` | no |
| `rev-parse` | `72` | `0` | `24` | `docs/cli/matrices/rev_parse_v2_47.tsv` | `52` | `46` | `0` | `0` | `6` | no |
| `show-index` | `1` | `0` | `0` | `docs/cli/matrices/show_index_v2_47.tsv` | `1` | `0` | `0` | `0` | `1` | no |
| `show-ref` | `14` | `0` | `5` | `docs/cli/matrices/show_ref_v2_47.tsv` | `10` | `7` | `0` | `0` | `3` | no |
| `sparse-checkout` | `11` | `0` | `0` | `docs/cli/matrices/sparse_checkout_v2_47.tsv` | `1` | `0` | `0` | `0` | `1` | no |
| `submodule` | `35` | `0` | `0` | `docs/cli/matrices/submodule_v2_47.tsv` | `7` | `0` | `0` | `0` | `7` | no |
| `symbolic-ref` | `8` | `0` | `3` | `docs/cli/matrices/symbolic_ref_v2_47.tsv` | `8` | `7` | `0` | `0` | `1` | no |
| `tag` | `40` | `0` | `11` | `docs/cli/matrices/tag_v2_47.tsv` | `27` | `17` | `0` | `0` | `10` | no |
| `verify-pack` | `4` | `0` | `0` | `docs/cli/matrices/verify_pack_v2_47.tsv` | `2` | `0` | `0` | `0` | `2` | no |
| `version` | `2` | `0` | `2` | `docs/cli/matrices/version_v2_47.tsv` | `4` | `2` | `0` | `0` | `2` | no |
| `worktree` | `28` | `0` | `0` | `docs/cli/matrices/worktree_v2_47.tsv` | `1` | `0` | `0` | `0` | `1` | no |

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

The latest `fetch --recurse-submodules` slices closed local/file no-submodule
value forms for implicit yes, explicit yes, boolean true, numeric true,
on-demand, no, boolean false, numeric false and `--no-recurse-submodules`,
plus stock-shaped invalid-value failure for `--recurse-submodules=bad`.
They also closed initialized local/file implicit yes, explicit yes, boolean
true, numeric true, on-demand, no, boolean false, numeric false and
`--no-recurse-submodules` forms for a changed submodule gitlink, plus
implicit yes, explicit yes and on-demand forms for a smart HTTP parent fetch
whose initialized submodule remote is local; that smart HTTP initialized case
now also covers boolean true, numeric true, no, boolean false, numeric false
and `--no-recurse-submodules` value forms. The latest uninitialized slice now
also covers explicit yes, boolean true, numeric true, no, boolean false,
numeric false and `--no-recurse-submodules` in addition to implicit yes and
on-demand for a smart HTTP parent fetch with the submodule still
uninitialized locally. The latest nested slice also closes
implicit yes for a smart HTTP
parent with initialized nested local submodule remotes:
Zmin fetches the target submodule commit into the initialized submodule object
database without checking it out for initialized submodules, recurses into
initialized nested submodules, and leaves uninitialized submodules
uninitialized, matching stock Git. The latest `fetch --jobs` slice closes
accepted `--jobs=2` and `-j -1` combinations with smart HTTP parent/local
submodule recursion, plus stock-shaped invalid non-integer diagnostics for
`--jobs` and `-j`. The latest transport expansion adds stock-oracle rows for
`--recurse-submodules=on-demand` with SSH and git-daemon parent remotes plus
initialized local submodule remotes. Non-local submodule remotes remain open.
The latest dry-run slice closes smart HTTP parent/local-submodule `--dry-run`
with default/on-demand recursion and explicit `--recurse-submodules`: parent
remote-tracking refs and `FETCH_HEAD` remain unchanged while the changed
submodule object is still fetched, matching stock Git.
The latest `fetch --server-option` slice closed equals, separate-value and
repeated protocol-v2 smart HTTP and SSH branch rows: Zmin now sends
`Git-Protocol: version=2` for smart HTTP, sets `GIT_PROTOCOL=version=2` for
SSH upload-pack, forwards all server-option values during both `ls-refs` and
`fetch`, and writes the same remote-tracking ref and `FETCH_HEAD` as stock Git.
The remaining written fetch open row is non-local submodule recursion.
The latest `fetch --upload-pack` slices closed equals and separate-value forms
for named local path and file URL remotes, configured fetch, explicit branch
`FETCH_HEAD` modes, multiple explicit refspecs, local/file `--all`,
local/file `--multiple` acceptance, local/file explicit-branch `--depth=1`,
local/file explicit-branch `--deepen=1`, local/file `--unshallow` forms and
local/file explicit-branch `--shallow-since` and `--shallow-exclude` forms in
existing shallow repos.
The latest `fetch --update-shallow` slices closed named local path/file URL
remotes plus explicit local path/file URL branch fetches where the source
remote itself is shallow. The latest update-shallow slices also closed explicit
local path/file URL HEAD fetches, multiple explicit refspec forms for named
local/file remotes and explicit local/file locations from shallow sources,
named branch fetches over smart HTTP, SSH and git daemon, network
multi-refspec forms over smart HTTP, SSH and git daemon, and branchless
configured fetch over smart HTTP, SSH and git daemon.
The latest `fetch --shallow-since` slices closed explicit local path/file URL
branch and HEAD fetches for equals and separate-value forms. The latest
`fetch --shallow-since` slice also closed multiple explicit refspec forms for
named local/file remotes, explicit local/file locations, and named remotes over
smart HTTP, SSH and git daemon, plus branchless configured fetch over smart
HTTP, SSH and git daemon. Related modes remain open until represented by
explicit rows.
The latest `fetch --shallow-exclude` slices closed explicit local path/file URL
branch and HEAD fetches for equals and separate-value forms, plus repeated
exclude forms for named local/file remote branch fetches, explicit local/file
branch fetches and explicit local/file HEAD fetches. The latest
`fetch --shallow-exclude` slice also closed multiple explicit refspec forms for
named local/file remotes, explicit local/file locations, and named remotes over
smart HTTP, SSH and git daemon, plus branchless configured fetch over smart
HTTP, SSH and git daemon. Related modes remain open until represented by
explicit rows.
The latest `fetch --deepen` slices closed explicit local path/file URL branch
and HEAD fetches for equals and separate-value forms. The latest deepen slice
also closed multiple explicit refspec forms for named local/file remotes and
explicit local/file locations in existing shallow repos. The latest network
slices closed branch deepen fetches, multiple explicit refspec fetches and
branchless configured fetches over smart HTTP, SSH and git daemon using
shallow boundary lines plus the `deepen-relative` capability. Related modes
remain open until represented by explicit rows.
The latest `fetch --unshallow` slices closed explicit local path/file URL HEAD
and branch fetches for existing shallow repos. The latest unshallow slice also
closed multiple explicit refspec forms for named local/file remotes and
explicit local/file locations in existing shallow repos. The latest network
slice closed branch, multiple explicit refspec and branchless configured
unshallow fetches over smart HTTP, SSH and git daemon using shallow boundary
lines plus an absolute deepen request. Related unmatrixed modes still need
expansion before `fetch` can be complete.
Zmin invokes the external upload-pack command where stock Git does for local
path and file URL forms, preserves stock Git's local/file `--all` and
`--multiple` behavior where the custom upload-pack command is not invoked, and
matches SSH upload-pack override for depth, deepen and unshallow branch fetches.

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
