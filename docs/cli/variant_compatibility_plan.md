# Variant Compatibility Plan

Command-name coverage is not full Git compatibility. A supported item must be
counted as a variant:

`command + option + value + option combination + repository state + transport/workflow + platform`.

Examples:

- `blame --date=iso` and `blame --date=relative` are two variants.
- `fetch --depth=1 <remote> <refspec>` and `fetch --depth=1 <remote> <refspec> <refspec>` are separate variants.
- Parser acceptance does not count. The behavior must match stock Git output,
  exit code and repository state.

## Counting Rules

- Count only stock-Git-supported behavior unless there is an explicit Zmin-only
  command.
- Do not count corrupt repository formats, reftable, Git LFS, legacy external
  bridges or package-manager install channels as closed unless they are
  implemented and tested.
- A variant is closed only with focused parity evidence: local compat test,
  upstream Git test slice or dogfood reproduction.
- Public docs may show command-name presence. They must not call it full support.

## Current Burn-Down

| Block | Closed | Open | Evidence |
| --- | ---: | ---: | --- |
| `blame --date` stock modes | `11` | `0` | `git_history_query_compat` |
| `blame --date` newly closed modes | `2` | `0` | `relative`, `human` |
| `blame` normal display flags | `4` | `0` | `-s`, `-t`, `-b`, `-c` |
| `blame` no-toggle/reset forms | `15` | `0` | standalone `--no-*` plus positive-then-no forms, excluding nuanced `--root --no-root` |
| `blame --no-abbrev` forms | `3` | `0` | `--no-abbrev`, `--abbrev=N --no-abbrev`, `--no-abbrev --abbrev=N` |
| `blame` final-disabled mode toggles | `10` | `0` | `--no-progress`, `--no-score-debug`, `--no-color-lines`, `--no-color-by-age`, `--no-minimal` plus positive-then-no forms |
| `blame -L` extended range forms | `6` | `0` | `N,-M`, `,N`, `/re/,-N`, `N,/re/`, `/re/,/re/`, `^/re/` |
| `blame -L :name` plain symbol boundary | `1` | `0` | stops at the matching non-function line like stock Git instead of extending to EOF |
| `init` quiet forms | `2` | `0` | `-q`, `--quiet` |
| `notes add` empty editor forms | `2` | `0` | `--allow-empty`, `--allow-empty --no-edit` |
| `notes copy` stdin/no-stdin toggle forms | `2` | `0` | `--stdin --no-stdin`, `--no-stdin --stdin` |
| `notes edit` message-source forms | `8` | `0` | `-m`, `--message=`, `-F`, `--file=`, `-C`, `--reuse-message=`, `-c`, `--reedit-message=` |
| `notes edit` compact short source forms | `4` | `0` | `-mmsg`, `-Ffile`, `-C<object>`, `-c<object>` |
| `notes merge` no-strategy toggle forms | `7` | `0` | merge order variants plus `--commit`/`--abort` state variants |
| `notes remove` stdin/no-stdin toggle forms | `2` | `0` | `--stdin --no-stdin`, `--no-stdin --stdin` |
| `clean` no-interactive toggle forms | `3` | `0` | `--no-interactive -n`, `-n --no-interactive`, `--interactive --no-interactive -n` |
| `column --mode` dense layout forms | `4` | `0` | `dense`, `nodense`, `column,dense`, `row,dense` |
| `log --decorate` boolean value forms | `5` | `0` | `yes`, `on`, `1`, `off`, `0` |
| `log --diff-merges` separate stat forms | `3` | `0` | `separate`, `on`, `m`; skip empty parent diff blocks like stock Git |
| `stash list` reflog/signature format atoms | `6` | `0` | `%gN`, `%gE`, `%gn`, `%ge`, `%GS`, `%GG` |
| `stash list` literal-preserved format atoms | `12` | `0` | `%r`, `%R`, `%q`, `%Q`, `%z`, `%gL`, `%gI`, `%gq`, `%gZ`, `%aZ`, `%cZ`, `%GZ` |
| `stash list` non-forced color format atoms | `3` | `0` | `%Cred`, `%C(red)`, `%C(auto,red)` with reset forms in redirected output |
| `stash list` forced color format atoms | `3` | `0` | `%C(always,red)`, `%C(always,bold red)`, `%C(always,blue)` with reset/normal forms |
| `stash list` width format atoms | `6` | `0` | `%<(N)`, `%>(N)`, `%<(N,trunc)`, `%>(N,trunc)`, `%<(N,ltrunc)`, `%<(N,mtrunc)` |
| `status -z` implicit porcelain form | `1` | `0` | `git status -z` matches stock Git's NUL-terminated porcelain v1 output |
| `status` option evidence forms | `5` | `0` | `--null`, `--short`, `-unormal`, bare `--untracked-files`, `--ignored=traditional` |
| `status` ahead-behind toggles | `2` | `0` | `--ahead-behind`, `--no-ahead-behind` with porcelain v1/v2 and equal/different upstream refs |
| `status` stash display toggles | `2` | `0` | `--show-stash`, `--no-show-stash` with human, porcelain v2 and toggle order |
| `status` long mode toggles | `2` | `0` | `--long`, `--no-long` with short/long order-sensitive forms |
| `status` verbose modes | `2` | `0` | `--verbose`, `--no-verbose`, `-v`, `-vv`, reset order and machine-readable combinations |
| `status` column modes | `2` | `0` | `--column`, `--no-column`, `--column=always/never`, `column.status=always` and machine-readable combinations |
| `status` global no-op option | `1` | `0` | `--no-optional-locks status --short` via leading global option parser |
| `status` rename modes | `4` | `0` | `--renames`, `--no-renames`, `--find-renames`, `--find-renames=<n>` for staged exact rename output |
| `status` ignore-submodules modes | `4` | `0` | `--ignore-submodules`, `=all`, `=dirty`, `=untracked` with dirty, untracked and new-commit submodule states |
| `status` human branch modes | `1` | `0` | `-b`/`--branch` standalone human status for dirty, untracked and upstream-ahead states |
| `status` pathspec modes | `13` | `0` | exact file, directory, default glob, explicit magic, exclude magic, human output and global pathspec flags |
| `for-each-ref` date format atoms | `16` | `0` | `committerdate` and `taggerdate` in default, `unix`, `raw`, `iso`, `iso-strict`, `rfc`, `rfc2822`, and `short` formats |
| `reflog expire` default policy forms | `6` | `0` | empty args, `main`, `HEAD`, `--updateref main`, `--rewrite main`, `--verbose main` |
| `reflog --date` display modes | `8` | `0` | `default`, `local`, `iso-strict`, `rfc`, `rfc2822`, `short`, `relative`, `human` |

Tracked closed blocks in this table: `193` verified variants.

This is closed evidence only, not the full Git denominator. A denominator is
valid only after the matching command group is expanded into command plus
option plus value plus option combination plus repository state plus transport
workflow plus platform.

The global denominator is still being audited. Until then, do not publish a
global compatibility percentage.

## Hard-Fail Scan

Raw scan from 2026-06-19:

`rg -n "unsupported|not supported yet|not implemented yet" crates/zmin-cli/src crates/zmin-git-core/src --glob '*.rs'`

This found `132` code hits. This is not the variant denominator. Each hit must
be classified as one of:

- Git-supported user variant to implement and test
- parser validation for invalid input
- corrupt or unsupported repository/storage format
- intentionally external or legacy integration
- additive Zmin-only behavior

Largest raw clusters:

| Area | Raw hits | Next action |
| --- | ---: | --- |
| `worktree_impl.rs` | `27` | split `clean`, `ls-files`, submodule, sparse-checkout, stash format atoms |
| `history_impl.rs` | `25` | split blame ranges/options, reflog formats, diff/log decorators, filter-branch |
| `transport_impl.rs` | `20` | split explicit-location fetch, remote helpers, reftable, HTTP/env guards |
| `notes_impl.rs` | `7` | split notes copy/edit/add/remove/prune/merge option gaps |
| `pack.rs` / `pack_impl.rs` | `11` | classify pack/bundle/commit-graph format guards versus stock-supported variants |
| `admin_impl.rs` | `5` | classify hook validation and legacy foreign-SCM adapters |
| remaining files | `39` | classify small parser/runtime guards individually |

## Audit Order

1. Local git-replacement blockers from IDE/GUI dogfood.
2. Option inventory from Git `v2.47.1` docs using
   `tools/git-compat-option-inventory.sh`.
3. Commands with live `unsupported` branches that stock Git accepts, expanded
   into option/mode/state variants before implementation.
4. High-use porcelain variants: `status`, `add`, `commit`, `diff`, `log`,
   `blame`, `stash`, `branch`, `checkout`, `switch`, `restore`.
5. Transport variants: local/file, smart HTTP, SSH, git daemon, depth,
   explicit refspecs, tags, prune, proxy/auth.
6. Plumbing variants used by tools: `cat-file`, `rev-parse`, `for-each-ref`,
   `ls-files`, `update-index`, `read-tree`, `write-tree`.
7. Platform variants: macOS, Linux, Windows path/process behavior.

## Reporting Format

For each slice, report:

`block: closed/total variants`

Do not mix command counts with variant counts.
