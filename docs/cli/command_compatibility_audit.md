# Command Compatibility Audit

This file is the compact summary of the current Git command entry-point
surface.

## Baselines

- Git `v2.32.0`: `145/145` tracked command entry points present
- Git `v2.47.1`: `151/151` tracked command entry points present
- complete Git `v2.47.1` behavior matrices: `0/151`
- complete documented command-option behavior matrices: `0/4632`
- commands with any written behavior matrix rows: `2/151`
- documented command-option pairs represented by at least one row: `50/4632`
  audit progress only, not option support

## Current state

- no missing commands for the tracked baselines
- current live `v2.47` report: `203` implemented commands, `151` matching
  the selected baseline, `0` missing, and `52` additive Zmin commands
- compatibility report is generated from the live CLI schema, not from a hand-maintained list
- extra commands stay visible in the report as additive surface, not as baseline failures
- command inventory parity means the command name exists; it is not a claim
  that every option, option value, option combination, repository state or
  transport workflow matches stock Git
- variant parity is tracked separately in
  `docs/cli/variant_compatibility_plan.md`
- option inventory starts from `tools/git-compat-option-inventory.sh`, then
  expands into behavior variants in `docs/cli/git_compatibility_inventory.md`

## Hard-fail inventory

`unsupported`, `not supported yet`, and `not implemented yet` call sites are
tracked as behavior gaps unless they are parser validation for corrupt input,
unsupported repository formats, or intentionally external/legacy integrations.

Recently closed replacement gaps:

- root `--version`, `-v`, `version`, and `version --build-options` emit a
  Git-compatible version line while keeping the real Zmin version visible
- depth fetch with multiple explicit refspecs works across local/file, smart
  HTTP, SSH, and git-daemon remotes
- explicit local/file fetch forms used by clients update `FETCH_HEAD` and
  destination refs like stock Git
- `blame -p`, `blame --porcelain`, and `blame --line-porcelain` emit
  stock-compatible porcelain output for the supported blame surface
- common blame client flags `-f`, `-n`, `-e`, `-w`, `--abbrev=N`,
  `--date=iso`, and simple numeric `-L start,end` ranges match stock Git for
  the supported blame surface
- normal blame display flags `-s`, `-t`, `-b`, and `-c` match stock Git for
  the supported blame surface
- common blame `--no-*` toggles reset/default output like stock Git for the
  supported blame surface; `--root --no-root` remains a separate nuanced case
- `blame --no-abbrev` renders the full object id width and respects later
  `--abbrev=N` overrides like stock Git
- final-disabled blame mode toggles match stock Git for progress, score debug,
  color lines, color by age, and minimal modes; enabled modes remain open until
  their output behavior is implemented
- additional `blame -L` stock range forms now match for negative counts,
  omitted start, regex end bounds, regex-to-regex bounds, and `^/regex/`
- `blame -L :name` no longer extends plain symbol matches to EOF when stock Git
  treats the match as a single-line range
- `init -q` and `init --quiet` suppress initialization output like stock Git
- `log --diff-merges=combined` and `log --diff-merges=dense-combined` are
  accepted and render the matching combined diff form
- `log --decorate=true` is accepted as enabled short decoration
- `cat-file --filters` applies checkout EOL conversion and smudge filters for
  both `REV:path` and `--path=<path> <blob>` forms
- `cat-file --textconv` runs configured diff-driver textconv commands and
  falls back to raw blob output when no textconv driver applies
- `ls-files --recurse-submodules --ignored --cached --exclude-standard`
  follows stock Git's supported cached-ignored mode instead of rejecting all
  ignored recurse-submodule combinations
- `for-each-ref` supports stock date atoms for `committerdate` and
  `taggerdate` in default, `unix`, `raw`, `iso`, `iso-strict`, `rfc`,
  `rfc2822`, and `short` formats
- `for-each-ref` supports stock author atoms for commit refs:
  `authorname`, `authoremail`, and the same `authordate` formats
- `for-each-ref` supports stock tagger identity atoms for annotated tags:
  `taggername` and `taggeremail`
- `for-each-ref` supports stock committer identity atoms for commit refs:
  `committername` and `committeremail`
- `for-each-ref` supports stock `objectsize` for commit, annotated tag, blob
  and tree refs
- `for-each-ref` supports stock `--sort=objectsize` ordering for those refs
- `for-each-ref` supports stock `refname:lstrip=<n>` and
  `refname:rstrip=<n>` format and sort modifiers for valid integer values
- `for-each-ref` matches stock invalid integer diagnostics for
  `refname:lstrip=<n>` and `refname:rstrip=<n>` format and sort modifiers
- `for-each-ref` supports `creator` and `creatordate` for commit refs and
  annotated tag refs
- `for-each-ref` supports stock `objectname:short=<n>` abbreviation lengths
  for positive values covered by the matrix
- `for-each-ref` rejects non-positive and non-numeric `objectname:short=<n>`
  values with stock Git's fatal diagnostic
- `stash list --format` and `stash list --pretty=format:` support common
  stash reflog atoms used by clients: `%H`, `%h`, `%gd`, `%gD`, `%gs`, `%s`,
  `%%`, `%n`, and `%xNN`
- `stash list --format` also supports reflog identity atoms `%gN`, `%gE`,
  `%gn`, `%ge`, plus unsigned signature text atoms `%GS` and `%GG`
- `stash list --format` preserves simple unknown atoms as literals like stock
  Git for `%r`, `%R`, `%q`, `%Q`, `%z`, `%gL`, `%gI`, `%gq`, `%gZ`, `%aZ`,
  `%cZ`, and `%GZ`
- `reflog expire` applies the default `gc.reflogExpire` policy for current
  explicit refs and matches stock Git for empty args, branch refs, `HEAD`,
  `--updateref`, `--rewrite`, and `--verbose`
- `reflog --date` supports stock display modes `default`, `local`,
  `iso-strict`, `rfc`, `rfc2822`, `short`, `relative`, and `human`
- `notes copy --for-rewrite=<command>` follows the configured rewrite gate for
  exact `notes.rewriteRef` refs and implies stdin pair input
- `notes add --allow-empty` opens the configured editor and writes the edited
  note content like stock Git, including the `--no-edit` toggle form
- `notes edit` supports stock Git's deprecated message-source options
  `-m`, `-F`, `-C`, and `-c` plus their long forms and warning text
- `notes merge --no-strategy` resets strategy selection like stock Git,
  including order-sensitive merge and merge-state forms
- `clean --no-interactive` matches stock Git's non-interactive toggle order
  for dry-run clean flows
- `column --mode` now matches stock Git dense and nodense layout selection for
  `dense`, `nodense`, `column,dense`, and `row,dense`
- `log --decorate` accepts stock Git boolean value forms `yes`, `on`, `1`,
  `off`, and `0`
- `log --diff-merges=m` is accepted as the stock alias for separate merge
  diffs, and separate/on/m stat output skips empty parent diff blocks like
  stock Git
- `stash list --format` accepts common non-forced pretty color atoms
  `%Cred`, `%C(red)`, and `%C(auto,red)` with reset forms when output is not
  color-forced
- `stash list --format` emits stock ANSI sequences for forced pretty color
  atoms `%C(always,red)`, `%C(always,bold red)`, and `%C(always,blue)` with
  reset/normal forms
- `stash list --format` supports simple pretty width modifiers for left/right
  padding and `trunc`/`ltrunc`/`mtrunc` truncation on the next atom
- `status -z` matches stock Git's implicit NUL-terminated porcelain v1 output
  instead of printing human status
- `status --null`, `status --short`, `status -unormal`, bare
  `status --untracked-files`, and `status --ignored=traditional` now have exact
  stock-Git parity evidence
- `status --ahead-behind` and `status --no-ahead-behind` match stock Git branch
  headers for porcelain v1/v2 with equal and different upstream refs
- `status --show-stash` and `status --no-show-stash` match stock Git for human
  output, porcelain v2 output and order-sensitive toggle forms
- `status --long` and `status --no-long` match stock Git's long output and
  order-sensitive interaction with `--short`
- `status --verbose` and `status --no-verbose` match stock Git for `-v`,
  `-vv`, order-sensitive reset forms and machine-readable combinations
- `status --column` and `status --no-column` match stock Git for columnized
  human untracked output, `column.status=always`, reset forms and
  machine-readable combinations
- `status --untracked-cache`, `status --no-untracked-cache`,
  `status --split-index`, and `status --no-split-index` were reclassified as
  invalid input because stock Git `2.47.1` rejects them for `git status`
- global `--no-optional-locks status --short` matches stock Git through the
  shared leading-global-option parser
- `status --renames`, `status --no-renames`, `status --find-renames`, and
  `status --find-renames=<n>` match stock Git for staged exact renames across
  human, porcelain v1/v2, and short forms
- `status --ignore-submodules`, `status --ignore-submodules=all`,
  `status --ignore-submodules=dirty`, and
  `status --ignore-submodules=untracked` match stock Git for dirty, untracked
  and new-commit submodule states across human, porcelain v1/v2 and short forms
- `status -b` and `status --branch` match stock Git for standalone human
  dirty, untracked and upstream-ahead states
- `status` pathspec rows match stock Git for exact file, directory, default
  glob, explicit magic, exclude magic, human output and global pathspec flags

Current high-priority gap classes:

- Git replacement flow: remaining IDE/GUI command combinations discovered by
  local dogfood with `/Users/dschewchenko/.local/bin/git`.
- Status matrix: `docs/cli/matrices/status_v2_47.tsv` currently tracks `60`
  rows: `56` closed, `0` partial, `0` open and `4` invalid-input.
- Variant inventory: the 2026-06-19 raw hard-fail scan has `132`
  `unsupported` / `not supported yet` / `not implemented yet` code hits to
  classify before any global percentage is honest.
- Core Git behavior: selected `notes`, `stash`, `submodule`, `ls-files`, and
  history-format options.
- Repository/transport formats: reftable, non-core remote helpers, bundle edge
  cases, and rare archive/pack variants.
- External legacy integrations: `p4`, `svn`, `archimport`, `git gui`, and
  `git citool` subcommands.

Do not present command inventory parity as full option parity while this
inventory has user-visible hard-fails.

## Behavior parity

Behavior parity is tracked by local compatibility tests, smoke scripts, and the
selected upstream Git test-suite baseline in
`docs/git/upstream_compatibility_baseline.md`.

As of 2026-06-18, the selected upstream Git `standard` suite and the expanded
supported-surface `exhaustive` set are green on macOS and
Windows/Git-for-Windows through the local Parallels runner. This is still not a
claim of full upstream Git parity outside the supported and tested surface.
Current command inventory validation is green for tracked baselines:
`ZMIN_GIT_GAP_STRICT=1 ./tools/git-command-gap.sh`, `ZMIN_GIT_BASELINE=v2.47.1
./tools/git-command-gap.sh`, and `cargo test -p zmin-cli --test
compatibility_command -- --nocapture` all pass with zero missing baseline
commands.

## Additive Zmin surface

`zmin clone --worktree-first` and `zmin clone --instant` are
additive Zmin clone modes. They do not change default `zmin clone`
behavior. The current surface supports local repositories, smart HTTP remotes,
git-daemon remotes, and SSH remotes. It creates a canonical Git repository,
materializes the selected `HEAD` working tree first, writes
`zmin.worktreeFirst=true` to `.git/config`, and keeps normal `fetch` /
`pull --ff-only` behavior available for later hydration. Remote worktree-first
clones initially write refs only for objects they requested; a later
`fetch origin` hydrates additional branch and tag refs.

`--background-fetch` and `--demand-hydrate` are explicit Zmin-only options for
remote worktree-first clones. `--background-fetch` starts a detached
`fetch origin` after checkout and records the background-fetch config keys.
`--demand-hydrate` marks `remote.origin.promisor=true`, records the demand
hydrate config keys, and lets missing `HEAD` objects hydrate on object reads.
Both options reject non-worktree-first modes and local repositories instead of
falling back to a different clone strategy.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_clone_compat -- --nocapture`
- `cargo test -p zmin-cli --test git_transport_http_compat clone_instant_ --
  --nocapture` (`9/9`)
- `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_transport_http_compat
  clone_instant_smart_http_materializes_head_then_fetch_hydrates_refs`
- `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_transport_http_compat
  clone_instant_smart_http_background_fetch_hydrates_refs`
- `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_transport_http_compat
  clone_instant_smart_http_demand_hydrate_recovers_missing_head_objects`
- `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_transport_http_compat
  clone_instant_git_daemon_materializes_head_then_fetch_hydrates_refs`
- `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_transport_http_compat
  clone_instant_git_daemon_background_fetch_hydrates_refs`
- `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_transport_http_compat
  clone_instant_git_daemon_demand_hydrate_recovers_missing_head_objects`
- `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_transport_http_compat
  clone_instant_ssh_materializes_head_then_fetch_hydrates_refs`
- `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_transport_http_compat
  clone_instant_ssh_background_fetch_hydrates_refs`
- `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_transport_http_compat
  clone_instant_ssh_demand_hydrate_recovers_missing_head_objects`
- `tools/parallels-windows-runner.sh validate targeted git_clone_compat
  clone_instant_local_repo_fetch_and_pull_remain_canonical_git_operations`
- `tools/parallels-windows-runner.sh validate targeted git_clone_compat
  clone_instant_local_repo_marks_worktree_first_without_changing_git_state`
- `tools/parallels-windows-runner.sh validate targeted git_clone_compat
  clone_worktree_first_rejects_non_worktree_or_remote_modes`

`zmin hooks` is additive Zmin porcelain; it does not replace or change the
Git-compatible `git hook run` command. The first managed hooks slice supports:

- `zmin hooks init`
- `zmin hooks add [--force] <hook> <command>`
- `zmin hooks list`
- `zmin hooks remove <hook>`

Supported hook names are `pre-commit`, `commit-msg`, `pre-push`,
`post-checkout`, and `post-merge`. Managed hooks store commands as multi-value
`.git/config` entries under `zmin.hooks.<hook>` and generate a standard
executable `.git/hooks/<hook>` shell runner that executes the commands in add
order, stops at the first failing command, and forwards normal hook arguments.
Existing non-managed hook files are not overwritten unless `--force` is passed.
With `--force`, Zmin takes ownership of the hook by replacing it with a
managed runner.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_admin_tools_compat hook -- --nocapture`
- `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  targeted git_admin_tools_compat
  managed_hooks_add_list_remove_and_protect_manual_hooks`

`zmin save`, `zmin changes`, `zmin publish`,
`zmin update`, `zmin undo`, `zmin timeline`, and
`zmin recover` are the first CMS-like porcelain commands. They are
additive and do not replace Git-compatible `add`, `commit`, `status`, `diff`,
`push`, `pull`, `reset`, `log`, `restore`, or reference behavior:

- `save <message>` runs the safe user workflow `add -A` plus `commit -m
  <message>` and reports a no-op when there is nothing to save.
- `changes` renders a short human-readable summary from porcelain status
  instead of exposing index/worktree terminology.
- `publish` refuses dirty worktrees and then runs the existing default `push`.
- `update` refuses dirty worktrees and then runs `pull --ff-only`.
- `undo` is operation-log backed and currently only undoes the last logged
  `save` when the worktree is clean and `HEAD` is still the saved commit. It
  uses the existing Git-compatible ref/reset behavior and leaves the undone
  file edits in the worktree.
- `timeline` is the human-readable history alias. It uses the existing `log`
  implementation, shows recent commits as short hash plus subject, and reports
  `No history.` for repositories without commits.
- `recover <path...>` is the human-readable safe file restore alias. It uses
  the existing `restore --worktree -- <path...>` behavior, refuses targets with
  staged/index changes, and reports each recovered path.

Validation:

- `cargo test -p zmin-cli --test git_cms_porcelain_compat -- --nocapture`
- `ZMIN_WINDOWS_VALIDATE_NO_FMT=1 tools/parallels-windows-runner.sh validate
  file git_cms_porcelain_compat` (`4/4`)

## Commands to run

```bash
ZMIN_GIT_GAP_STRICT=1 ./tools/git-command-gap.sh
ZMIN_GIT_BASELINE=v2.47.1 ./tools/git-command-gap.sh
cargo test -p zmin-cli --test compatibility_command
```
