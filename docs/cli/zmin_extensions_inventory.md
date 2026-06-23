# Zmin Extension Inventory

This inventory is separate from Git `2.47.1` compatibility.

Git-compatible rows measure stock Git behavior. Zmin extensions are additive
features exposed by `zmin` and must not increase command, option or behavior
coverage numbers in the Git compatibility matrix.

## Counts

| Layer | Count | Meaning |
| --- | ---: | --- |
| Zmin-only commands | `11` | additive top-level commands that are not Git command names |
| Zmin-only options on Git commands | `4` | additive options on existing Git-compatible commands |
| Zmin-only environment controls | `1` | additive environment variables for Zmin internals or transport tuning |
| Zmin-only schema command aliases | `6` | flattened schema entries that belong to Zmin-only command groups |
| Stable extensions | `5` | implemented and covered by focused tests |
| Experimental extensions | `2` | implemented but still preview-only |
| Planned extensions | `1` | designed backlog, not implemented |

## Zmin-Only Commands

| Command | Status | Evidence | Notes |
| --- | --- | --- | --- |
| `zmin hooks` | stable | `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks`; `git_admin_tools_compat::managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation` | supports `init`, `add [--force]`, `list`, and `remove` managed-hook subcommands for supported hook names; rejects unsupported managed-hook names as Zmin-only validation |
| `zmin repo` | stable | `git_admin_tools_compat::repo_command_is_tracked_zmin_only_extension` | exposes Zmin-only repository metadata and structure summaries; stock Git has no `git repo` command |
| `zmin diff-pairs` | stable | `git_diff_compat::diff_pairs_matches_stock_git_for_raw_diff_input` | consumes raw `git diff-tree -z -r --raw` input on stdin and renders selected diff formats; stock Git has no `git diff-pairs` command, so this is tracked outside the Git `2.47.1` denominator |
| `zmin last-modified` | stable | `git_history_query_compat::last_modified_reports_latest_commit_per_path` | reports the latest commit that affected each selected path, with recursive and NUL-delimited modes; stock Git has no `git last-modified` command, so this is tracked outside the Git `2.47.1` denominator |
| `zmin save <message>` | experimental | `git_cms_porcelain_compat` | CMS-style `add -A` plus `commit -m` wrapper |
| `zmin changes` | experimental | `git_cms_porcelain_compat` | human-readable status wrapper |
| `zmin publish` | experimental | `git_cms_porcelain_compat` | safe push wrapper |
| `zmin update` | experimental | `git_cms_porcelain_compat` | safe pull wrapper |
| `zmin undo` | experimental | `git_cms_porcelain_compat` | operation-log backed undo for the last clean `save` |
| `zmin timeline` | experimental | `git_cms_porcelain_compat` | human-readable history wrapper |
| `zmin recover` | experimental | `git_cms_porcelain_compat` | safe file restore wrapper |

## Zmin-Only Schema Command Aliases

These rows map flattened `zmin compat` schema names back to additive command
groups. They are machine-readable classification rows only; they do not add
Git `2.47.1` compatibility coverage.

| Command | Status | Evidence | Notes |
| --- | --- | --- | --- |
| `zmin hooks init` | stable | `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks` | flattened schema alias `hooks-init`; initializes managed-hook metadata without replacing manual hooks |
| `zmin hooks add` | stable | `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks`; `git_admin_tools_compat::managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation` | flattened schema alias `hooks-add`; adds managed hook commands and validates supported Zmin hook names |
| `zmin hooks list` | stable | `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks` | flattened schema alias `hooks-list`; lists configured managed-hook commands |
| `zmin hooks remove` | stable | `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks` | flattened schema alias `hooks-remove`; removes managed hook commands without deleting manual hooks |
| `zmin repo info` | stable | `git_admin_tools_compat::repo_command_is_tracked_zmin_only_extension` | flattened schema alias `repo-info`; reports Zmin-only repository metadata |
| `zmin repo structure` | stable | `git_admin_tools_compat::repo_command_is_tracked_zmin_only_extension` | flattened schema alias `repo-structure`; reports Zmin-only repository layout summaries |

## Zmin-Only Options

| Command | Option | Status | Evidence | Notes |
| --- | --- | --- | --- | --- |
| `zmin clone` | `--worktree-first` | stable | `git_clone_compat::clone_instant_local_repo_marks_worktree_first_without_changing_git_state`; `git_clone_compat::clone_worktree_first_rejects_non_worktree_or_remote_modes` | materializes selected `HEAD` first and records `zmin.worktreeFirst=true` |
| `zmin clone` | `--instant` | stable | `git_transport_http_compat::clone_instant_git_daemon_materializes_head_then_fetch_hydrates_refs`; `git_transport_http_compat::clone_instant_ssh_materializes_head_then_fetch_hydrates_refs`; `git_transport_http_compat::clone_instant_smart_http_materializes_head_then_fetch_hydrates_refs` | alias for worktree-first clone mode over git-daemon, SSH and smart HTTP transport |
| `zmin clone` | `--background-fetch` | experimental | `git_transport_http_compat::clone_instant_git_daemon_background_fetch_hydrates_refs`; `git_transport_http_compat::clone_instant_ssh_background_fetch_hydrates_refs`; `git_transport_http_compat::clone_instant_smart_http_background_fetch_hydrates_refs` | starts a detached `fetch origin` after an instant remote clone |
| `zmin clone` | `--demand-hydrate` | experimental | `git_transport_http_compat::clone_instant_git_daemon_demand_hydrate_recovers_missing_head_objects`; `git_transport_http_compat::clone_instant_ssh_demand_hydrate_recovers_missing_head_objects`; `git_transport_http_compat::clone_instant_smart_http_demand_hydrate_recovers_missing_head_objects` | marks instant remote clones as promisor-backed for missing-object hydration |

## Zmin-Only Environment Controls

| Variable | Status | Evidence | Notes |
| --- | --- | --- | --- |
| `ZMIN_GIT_HTTP_VERSION` | stable | `transport_impl::tests::remote_http_helper_version_arg_rejects_unsupported_values` | selects the Zmin HTTP remote-helper protocol preference; accepted values are `auto`, `http1`, `http2` and `http3`; invalid values are Zmin-only validation and do not count toward Git `2.47.1` compatibility |

## Planned: Staged Hook Runner

The next hooks extension should stay Zmin-only and must not change standard Git
hook semantics.

Detailed command contract and acceptance rows live in
`docs/cli/zmin_hooks_staged_runner.md` and
`docs/cli/zmin_hooks_staged_runner_acceptance.tsv`.

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
   renamed, deleted and unstaged-only paths.
2. Add `hooks run <hook> --staged --list` as a non-executing preview and mark
   the matching acceptance rows with evidence.
3. Add extension and pathspec filters after the selector contract is stable.
4. Add command execution after selector parity is covered.
5. Add managed-hook wrapper integration so `pre-commit` can call the staged
   runner automatically.

This staged runner remains separate from Git compatibility reporting because
stock Git has no equivalent `git hooks run --staged` command.
