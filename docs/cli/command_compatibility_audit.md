# Command Compatibility Audit

This file is the compact summary of the current Git command surface.

## Baselines

- Git `v2.32.0`: `145/145` tracked commands present
- Git `v2.47.1`: `151/151` tracked commands present

## Current state

- no missing commands for the tracked baselines
- current live `v2.47` report: `203` implemented commands, `151` matching
  the selected baseline, `0` missing, and `52` additive Zmin commands
- compatibility report is generated from the live CLI schema, not from a hand-maintained list
- extra commands stay visible in the report as additive surface, not as baseline failures
- command inventory parity means the command name exists; it is not a claim
  that every option, mode, repository state or transport workflow matches stock
  Git
- variant parity is tracked separately in
  `docs/cli/variant_compatibility_plan.md`

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
- `stash list --format` and `stash list --pretty=format:` support common
  stash reflog atoms used by clients: `%H`, `%h`, `%gd`, `%gD`, `%gs`, `%s`,
  `%%`, `%n`, and `%xNN`
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

Current high-priority gap classes:

- Git replacement flow: remaining IDE/GUI command combinations discovered by
  local dogfood with `/Users/dschewchenko/.local/bin/git`.
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
