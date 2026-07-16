# Oracle Test Deferrals

This inventory is for focused tests that look like stock-oracle coverage but
must not be imported into Git `2.47.1` behavior matrices yet.

Each entry keeps the generated oracle backlog honest: the test is reviewed, but
it is not a closed Git `2.47.1` row.

| Evidence | Classification | Reason |
| --- | --- | --- |
| `compatibility_command::compatibility_profile_v2_47_keeps_current_acceptance_gate` | deferred | The test validates the generated `zmin compat --profile v2-47 --format json` acceptance/reporting gate for current schema counts and ready-command flags. It does not prove any exact Git `2.47.1` command/option/value/state/transport/platform behavior row, so keep it out of Git behavior matrices and track it only as reviewed non-denominator evidence. |
| `git_history_query_compat::whatchanged_requires_explicit_opt_in_like_git_2_54` | deferred | The test asserts Git `2.54` removal-warning behavior and does not compare stock Git `2.47.1` stdout, stderr, exit code and side effects. Keep it out of Git `2.47.1` matrices until a real `2.47.1` whatchanged oracle row is added. |

## Source Guard Deferrals

- `"error: migrating repositories with worktrees is not supported yet\n"` in
  `crates/zmin-cli/src/cli/commands/reference_impl.rs` is a stock-compatible
  `git refs migrate --ref-format=reftable --dry-run` linked-worktree rejection,
  verified for exact stdout, stderr and exit 255 by
  `git_refs_compat::refs_migrate_rejects_linked_worktrees_like_stock_git` and
  classified as `invalid-input` in `refs_v2_47.tsv`.

- `"unsupported reftable version"` in `crates/zmin-git-core/src/refs.rs` is a
  corrupt/internal reftable parse guard. It is documented and intentionally
  kept outside the Git `2.47.1` exact behavior denominator until a dedicated
  invalid-repository parity row exists.
- `"unsupported reftable ref value type"` in
  `crates/zmin-git-core/src/refs.rs` is a corrupt/internal reftable parse
  guard. It is documented and intentionally kept outside the Git `2.47.1`
  exact behavior denominator until a dedicated invalid-repository parity row
  exists.
- `"unsupported built-in zmin lfs pull remote: {url}"` in
  `crates/zmin-cli/src/cli/commands/lfs_impl.rs` is a deliberate local-scope
  guard on the current built-in Git LFS foundation. The covered LFS replace-git
  surface currently includes discovery commands, hook takeover, pointer
  checkout, and local/file-based pull lanes, but not general network,
  authenticated, or custom-transfer Git LFS transport parity. Keep this guard
  outside the Git `2.47.1` denominator and classify the broader non-local LFS
  work under the deferred `git lfs` surface in
  `docs/cli/zmin_extensions_inventory.md` until that transport scope is
  implemented and verified.
