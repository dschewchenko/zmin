# Command Compatibility Audit

This file is the compact public summary of the current Git command surface.

## Baselines

- Git `v2.32.0`: `145/145` tracked commands present
- Git `v2.47.1`: `151/151` tracked commands present

## Current state

- no missing commands for the tracked baselines
- current live `v2.47` report: `203` implemented commands, `151` matching
  the selected baseline, `0` missing, and `52` additive Zmin commands
- compatibility report is generated from the live CLI schema, not from a hand-maintained list
- extra commands stay visible in the report as additive surface, not as baseline failures
- command inventory parity means the command surface is present; it is not a
  claim that every command matches stock Git behavior for every option and edge
  case

## Behavior parity

Behavior parity is tracked by local compatibility tests, smoke scripts, and the
selected upstream Git test-suite baseline in
`docs/git/upstream_compatibility_baseline.md`.

As of 2026-06-16, the selected upstream Git `standard` suite and the expanded
supported-surface `exhaustive` set are green on macOS and
Windows/Git-for-Windows through the local Parallels runner. This is still not a
claim of full upstream Git parity outside the supported and tested surface.

## Additive Zmin surface

`zmin clone --worktree-first` and `zmin clone --instant` are
additive Zmin clone modes. They do not change default `zmin clone`
behavior. The first slice supports local repositories only: it creates a
canonical Git repository, materializes the worktree, keeps full local history
available, and writes `zmin.worktreeFirst=true` to `.git/config`. Local
worktree-first repositories keep using canonical `fetch` and `pull --ff-only`
behavior and preserve the marker across updates. Smart HTTP, SSH, and
git-daemon worktree-first hydration are not implemented yet and fail fast
instead of silently falling back to a normal clone.

Validation:

- `cargo fmt --all -- --check`
- `cargo test -p zmin-cli --test git_clone_compat -- --nocapture`
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
