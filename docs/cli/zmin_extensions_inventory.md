# Zmin Extension Inventory

This inventory is separate from Git `2.47.1` compatibility.

Git-compatible rows measure stock Git behavior. Zmin extensions are additive
features exposed by `zmin` and must not increase command, option or behavior
coverage numbers in the Git compatibility matrix.

## Counts

| Layer | Count | Meaning |
| --- | ---: | --- |
| Zmin-only commands | `12` | additive top-level commands that are not Git command names |
| Zmin-only options on Git commands | `15` | additive options on existing Git-compatible commands |
| Zmin-only environment controls | `1` | additive environment variables for Zmin internals or transport tuning |
| Zmin-only schema command aliases | `10` | flattened schema entries that belong to Zmin-only command groups |
| Deferred/non-Git-2.47 schema commands | `1` | schema commands compared to newer/current stock Git but outside the Git `2.47.1` denominator |
| Stable extensions | `6` | implemented and covered by focused tests |
| Experimental extensions | `2` | implemented but still preview-only |
| Planned extensions | `0` | designed backlog, not implemented |

## Zmin-Only Commands

| Command | Status | Evidence | Notes |
| --- | --- | --- | --- |
| `zmin hooks` | stable | `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks`; `git_admin_tools_compat::managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation`; `git_admin_tools_compat::managed_hooks_run_staged_list_uses_index_backed_selector`; `git_admin_tools_compat::managed_hooks_run_staged_ext_list_and_execution_use_selected_paths`; `git_admin_tools_compat::managed_hooks_run_staged_dry_run_and_exit_code_match_command_mode_contract`; `git_admin_tools_compat::managed_hooks_run_staged_pathspec_filters_list_dry_run_and_execution`; `git_admin_tools_compat::managed_hooks_staged_runner_wrapper_integrates_with_pre_commit_workflow` | supports `init`, `add [--force]`, `list`, `remove`, and index-backed `run <hook> --staged` preview/execution with extension filters, pathspec filters, dry-run output, child exit-code propagation, and managed `pre-commit` staged-runner wrappers for supported hook names; rejects unsupported managed-hook names as Zmin-only validation |
| `zmin repo` | stable | `git_admin_tools_compat::repo_command_is_tracked_zmin_only_extension` | exposes Zmin-only repository metadata and structure summaries; stock Git has no `git repo` command |
| `zmin diff-pairs` | stable | `git_diff_compat::diff_pairs_matches_stock_git_for_raw_diff_input` | consumes raw `git diff-tree -z -r --raw` input on stdin and renders selected diff formats; stock Git has no `git diff-pairs` command, so this is tracked outside the Git `2.47.1` denominator |
| `zmin last-modified` | stable | `git_history_query_compat::last_modified_reports_latest_commit_per_path` | reports the latest commit that affected each selected path, with recursive and NUL-delimited modes; stock Git has no `git last-modified` command, so this is tracked outside the Git `2.47.1` denominator |
| `zmin history` | experimental | `git_admin_tools_compat::history_reword_dry_run_prints_ref_updates_without_moving_branch`; `git_admin_tools_compat::history_split_dry_run_splits_selected_file_hunks`; `git_admin_tools_compat::history_split_pathspec_can_select_all_matching_hunks` | additive history rewrite workflow with `reword` and `split` dry-run coverage; stock Git `2.47.1` has no `git history` command, so this is tracked outside the compatibility denominator |
| `zmin save <message>` | experimental | `git_cms_porcelain_compat::cms_changes_and_save_compose_existing_git_operations` | CMS-style `add -A` plus `commit -m` wrapper |
| `zmin changes` | experimental | `git_cms_porcelain_compat::cms_changes_and_save_compose_existing_git_operations` | human-readable status wrapper |
| `zmin publish` | experimental | `git_cms_porcelain_compat::cms_publish_and_update_use_safe_remote_operations` | safe push wrapper |
| `zmin update` | experimental | `git_cms_porcelain_compat::cms_publish_and_update_use_safe_remote_operations` | safe pull wrapper |
| `zmin undo` | experimental | `git_cms_porcelain_compat::cms_undo_reverts_last_logged_save_only_when_safe` | operation-log backed undo for the last clean `save` |
| `zmin timeline` | experimental | `git_cms_porcelain_compat::cms_timeline_and_recover_are_safe_human_aliases` | human-readable history wrapper |
| `zmin recover` | experimental | `git_cms_porcelain_compat::cms_timeline_and_recover_are_safe_human_aliases` | safe file restore wrapper |

## Zmin-Only Schema Command Aliases

These rows map flattened `zmin compat` schema names back to additive command
groups. They are machine-readable classification rows only; they do not add
Git `2.47.1` compatibility coverage.

| Command | Status | Evidence | Notes |
| --- | --- | --- | --- |
| `zmin hooks init` | stable | `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks` | flattened schema alias `hooks-init`; initializes managed-hook metadata without replacing manual hooks |
| `zmin hooks add` | stable | `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks`; `git_admin_tools_compat::managed_hooks_reject_unsupported_hook_names_as_zmin_extension_validation`; `git_admin_tools_compat::managed_hooks_staged_runner_wrapper_integrates_with_pre_commit_workflow` | flattened schema alias `hooks-add`; adds managed hook commands, supports managed staged-runner wrappers for supported hook names, and validates supported Zmin hook names |
| `zmin hooks list` | stable | `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks` | flattened schema alias `hooks-list`; lists configured managed-hook commands |
| `zmin hooks remove` | stable | `git_admin_tools_compat::managed_hooks_add_list_remove_and_protect_manual_hooks`; `git_admin_tools_compat::managed_hooks_staged_runner_wrapper_integrates_with_pre_commit_workflow` | flattened schema alias `hooks-remove`; removes managed hook commands or staged-runner config without deleting manual hooks |
| `zmin hooks run` | stable | `git_admin_tools_compat::managed_hooks_run_staged_list_uses_index_backed_selector`; `git_admin_tools_compat::managed_hooks_run_staged_ext_list_and_execution_use_selected_paths`; `git_admin_tools_compat::managed_hooks_run_staged_dry_run_and_exit_code_match_command_mode_contract`; `git_admin_tools_compat::managed_hooks_run_staged_pathspec_filters_list_dry_run_and_execution`; `git_admin_tools_compat::managed_hooks_staged_runner_wrapper_integrates_with_pre_commit_workflow` | flattened schema alias `hooks-run`; current implemented surface includes preview, extension filters, pathspec filters, dry-run output, direct command execution over selected staged paths, and wrapper-driven pre-commit execution |
| `zmin repo info` | stable | `git_admin_tools_compat::repo_command_is_tracked_zmin_only_extension` | flattened schema alias `repo-info`; reports Zmin-only repository metadata |
| `zmin repo structure` | stable | `git_admin_tools_compat::repo_command_is_tracked_zmin_only_extension` | flattened schema alias `repo-structure`; reports Zmin-only repository layout summaries |
| `zmin history reword` | experimental | `git_admin_tools_compat::history_reword_dry_run_prints_ref_updates_without_moving_branch` | flattened schema alias `history-reword`; dry-run prints planned ref updates without moving the branch |
| `zmin history split` | experimental | `git_admin_tools_compat::history_split_dry_run_splits_selected_file_hunks`; `git_admin_tools_compat::history_split_pathspec_can_select_all_matching_hunks` | flattened schema alias `history-split`; dry-run split planning supports selected file hunks and pathspec filtering |

## Deferred Non-Git-2.47 Schema Commands

These rows keep schema entries out of the Git `2.47.1` denominator when their
evidence compares against newer/current stock Git rather than Git `2.47.1`.

| Command | Status | Evidence | Notes |
| --- | --- | --- | --- |
| `zmin backfill` | deferred | `git_admin_tools_compat::backfill_matches_stock_git_for_complete_repository_noop`; `git_admin_tools_compat::backfill_promisor_remote_recovers_missing_local_objects` | local/current stock Git has `git backfill`, but Git `2.47.1` command-list does not; keep outside the Git `2.47.1` compatibility denominator until a target profile that includes `backfill` is active |

## Zmin-Only Options

| Command | Option | Status | Evidence | Notes |
| --- | --- | --- | --- | --- |
| `zmin clone` | `--worktree-first` | stable | `git_clone_compat::clone_instant_local_repo_marks_worktree_first_without_changing_git_state`; `git_clone_compat::clone_worktree_first_rejects_non_worktree_or_remote_modes` | materializes selected `HEAD` first and records `zmin.worktreeFirst=true` |
| `zmin clone` | `--instant` | stable | `git_clone_compat::clone_instant_local_repo_fetch_and_pull_remain_canonical_git_operations`; `git_transport_http_compat::clone_instant_git_daemon_materializes_head_then_fetch_hydrates_refs`; `git_transport_http_compat::clone_instant_ssh_materializes_head_then_fetch_hydrates_refs`; `git_transport_http_compat::clone_instant_smart_http_materializes_head_then_fetch_hydrates_refs` | alias for worktree-first clone mode over local repositories, git-daemon, SSH and smart HTTP transport; local instant clones keep later `fetch origin` and `pull --ff-only` as canonical Git operations while preserving `zmin.worktreeFirst=true` |
| `zmin clone` | `--background-fetch` | experimental | `git_transport_http_compat::clone_instant_git_daemon_background_fetch_hydrates_refs`; `git_transport_http_compat::clone_instant_ssh_background_fetch_hydrates_refs`; `git_transport_http_compat::clone_instant_smart_http_background_fetch_hydrates_refs` | starts a detached `fetch origin` after an instant remote clone |
| `zmin clone` | `--demand-hydrate` | experimental | `git_transport_http_compat::clone_instant_git_daemon_demand_hydrate_recovers_missing_head_objects`; `git_transport_http_compat::clone_instant_ssh_demand_hydrate_recovers_missing_head_objects`; `git_transport_http_compat::clone_instant_smart_http_demand_hydrate_recovers_missing_head_objects` | marks instant remote clones as promisor-backed for missing-object hydration |
| `zmin cat-file` | `--type` | stable | `manual stock oracle 2026-06-23: git cat-file --type exits 129; zmin cat-file --type maps to -t` | Zmin-only long alias for `cat-file -t`; stock Git `2.47.1` rejects this option, so it stays outside the Git compatibility denominator |
| `zmin cat-file` | `--size` | stable | `manual stock oracle 2026-06-23: git cat-file --size exits 129; zmin cat-file --size maps to -s` | Zmin-only long alias for `cat-file -s`; stock Git `2.47.1` rejects this option, so it stays outside the Git compatibility denominator |
| `zmin cat-file` | `--exists` | stable | `manual stock oracle 2026-06-23: git cat-file --exists exits 129; zmin cat-file --exists maps to -e` | Zmin-only long alias for `cat-file -e`; stock Git `2.47.1` rejects this option, so it stays outside the Git compatibility denominator |
| `zmin cat-file` | `--pretty` | stable | `manual stock oracle 2026-06-23: git cat-file --pretty exits 129; zmin cat-file --pretty maps to -p` | Zmin-only long alias for `cat-file -p`; stock Git `2.47.1` rejects this option, so it stays outside the Git compatibility denominator |
| `zmin imap-send` | `--folder` | stable | `manual stock oracle 2026-06-23: git imap-send --folder exits 129; zmin imap-send --folder selects the target mailbox` | Zmin-only mailbox override; stock Git `2.47.1` rejects this option, so it stays outside the Git compatibility denominator |
| `zmin imap-send` | `--list` | stable | `manual stock oracle 2026-06-23: git imap-send --list exits 129; zmin imap-send --list lists mailboxes` | Zmin-only mailbox listing mode; stock Git `2.47.1` rejects this option, so it stays outside the Git compatibility denominator |
| `zmin imap-send` | `-f` | stable | `manual stock oracle 2026-06-23: git imap-send -f exits 129; zmin imap-send -f aliases --folder` | Zmin-only short mailbox override; stock Git `2.47.1` rejects this option, so it stays outside the Git compatibility denominator |
| `zmin credential-cache` | `--daemon-internal` | internal | `manual stock oracle 2026-06-23: git credential-cache --daemon-internal exits 129; zmin credential-cache --daemon-internal --socket <path> starts the private cache daemon helper` | Zmin-only internal daemon helper used by the credential-cache implementation; stock Git `2.47.1` rejects this option, so it stays outside the Git compatibility denominator |
| `zmin instaweb` | `--daemon-internal` | internal | `tools/git-instaweb-local-oracle-gap-probe.sh::upstream_git_instaweb_daemon_internal_rejected` | Zmin-only internal daemon helper used to run the builtin `instaweb` server; upstream Git `2.47.1` rejects this option as unknown, so it stays outside the compatibility denominator |
| `zmin instaweb` | `--git-dir` | internal | `tools/git-instaweb-local-oracle-gap-probe.sh::upstream_git_instaweb_daemon_internal_rejected` | paired internal path option for the Zmin builtin `instaweb` daemon; upstream Git `2.47.1` only reaches an unknown-option failure on the preceding internal flag, so this stays outside the compatibility denominator |
| `zmin instaweb` | `--work-tree` | internal | `tools/git-instaweb-local-oracle-gap-probe.sh::upstream_git_instaweb_daemon_internal_rejected` | paired internal path option for the Zmin builtin `instaweb` daemon; stock Git `2.47.1` does not expose this internal surface, so it stays outside the compatibility denominator |

## Zmin-Only Environment Controls

| Variable | Status | Evidence | Notes |
| --- | --- | --- | --- |
| `ZMIN_GIT_HTTP_VERSION` | stable | `transport_impl::tests::remote_http_helper_version_arg_rejects_unsupported_values` | selects the Zmin HTTP remote-helper protocol preference; accepted values are `auto`, `http1`, `http2` and `http3`; invalid values are Zmin-only validation and do not count toward Git `2.47.1` compatibility |

## Staged Hook Runner

The staged hook runner is a Zmin-only extension and must not change standard
Git hook semantics.

Detailed command contract and acceptance rows live in
`docs/cli/zmin_hooks_staged_runner.md` and
`docs/cli/zmin_hooks_staged_runner_acceptance.tsv`.

Current implemented user-facing API:

```bash
zmin hooks run pre-commit --staged --list
zmin hooks run pre-commit --staged --ext rs,ts,js --list
zmin hooks run pre-commit --staged --list -- src
zmin hooks run pre-commit --staged --dry-run -- command ...
zmin hooks run pre-commit --staged -- command ...
zmin hooks run pre-commit --staged -- src -- command ...
```

Current selector contract:

- read staged paths from the index, not from the working tree
- list deleted paths distinctly while keeping the preview index-backed
- preserve renamed paths using the staged destination path
- return an empty successful preview when the index has no staged entries
- support extension filters before list, dry-run, or execution output is rendered
- support pathspec filters before list, dry-run, or execution output is rendered
- pass only selected staged executable files to command mode
- return the child exit code from command mode

Verified wrapper requirements:

- skip deleted paths by default during command execution, while still listing
  them in preview output
- work from a standard Git hook wrapper without breaking `.git/hooks/<hook>`
- keep managed hooks optional; manual hooks must still work

This staged runner remains separate from Git compatibility reporting because
stock Git has no equivalent `git hooks run --staged` command.
