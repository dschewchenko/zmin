# Zmin Extension Inventory

This inventory is separate from Git `2.47.1` compatibility.

Git-compatible rows measure stock Git behavior. Zmin extensions are additive
features exposed by `zmin` and must not increase command, option or behavior
coverage numbers in the Git compatibility matrix.

## Counts

| Layer | Count | Meaning |
| --- | ---: | --- |
| Zmin-only commands | `8` | additive top-level commands that are not Git command names |
| Zmin-only options on Git commands | `4` | additive options on existing Git-compatible commands |
| Stable extensions | `2` | implemented and covered by focused tests |
| Experimental extensions | `2` | implemented but still preview-only |
| Planned extensions | `1` | designed backlog, not implemented |

## Zmin-Only Commands

| Command | Status | Evidence | Notes |
| --- | --- | --- | --- |
| `zmin hooks` | stable | `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks` | supports `init`, `add [--force]`, `list`, and `remove` managed-hook subcommands |
| `zmin save <message>` | experimental | `git_cms_porcelain_compat` | CMS-style `add -A` plus `commit -m` wrapper |
| `zmin changes` | experimental | `git_cms_porcelain_compat` | human-readable status wrapper |
| `zmin publish` | experimental | `git_cms_porcelain_compat` | safe push wrapper |
| `zmin update` | experimental | `git_cms_porcelain_compat` | safe pull wrapper |
| `zmin undo` | experimental | `git_cms_porcelain_compat` | operation-log backed undo for the last clean `save` |
| `zmin timeline` | experimental | `git_cms_porcelain_compat` | human-readable history wrapper |
| `zmin recover` | experimental | `git_cms_porcelain_compat` | safe file restore wrapper |

## Zmin-Only Options

| Command | Option | Status | Evidence | Notes |
| --- | --- | --- | --- | --- |
| `zmin clone` | `--worktree-first` | stable | `git_clone_compat`, `git_transport_http_compat clone_instant_` | materializes selected `HEAD` first and records `zmin.worktreeFirst=true` |
| `zmin clone` | `--instant` | stable | `git_clone_compat`, `git_transport_http_compat clone_instant_` | alias for worktree-first clone mode |
| `zmin clone` | `--background-fetch` | experimental | `git_transport_http_compat clone_instant_` | starts a detached `fetch origin` after an instant remote clone |
| `zmin clone` | `--demand-hydrate` | experimental | `git_transport_http_compat clone_instant_` | marks instant remote clones as promisor-backed for missing-object hydration |

## Planned: Staged Hook Runner

The next hooks extension should stay Zmin-only and must not change standard Git
hook semantics.

Candidate user-facing API:

```bash
zmin hooks run pre-commit --staged
zmin hooks run pre-commit --staged --ext rs,ts,js
zmin hooks run pre-commit --staged --list
zmin hooks run pre-commit --staged --dry-run -- command ...
zmin hooks run pre-commit --staged -- command ...
```

Requirements:

- read staged paths from the index, not from the working tree
- support pathspec and extension filters
- skip deleted paths by default, while still listing them in dry-run output
- preserve renamed paths using the staged destination path
- pass only selected staged files to the command, not the whole project
- provide `--list` / `--dry-run` output before executing tools
- work from a standard Git hook wrapper without breaking `.git/hooks/<hook>`
- keep managed hooks optional; manual hooks must still work

Suggested implementation order:

1. Add an index-backed staged-file selector with tests for modified, added,
   renamed and deleted paths.
2. Add `hooks run <hook> --staged --list` as a non-executing preview.
3. Add command execution after selector parity is covered.
4. Add managed-hook wrapper integration so `pre-commit` can call the staged
   runner automatically.

This staged runner remains separate from Git compatibility reporting because
stock Git has no equivalent `git hooks run --staged` command.
