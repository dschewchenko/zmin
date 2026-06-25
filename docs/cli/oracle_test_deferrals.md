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

- `"unsupported reftable version"` in `crates/zmin-git-core/src/refs.rs` is a
  corrupt/internal reftable parse guard. It is documented and intentionally
  kept outside the Git `2.47.1` exact behavior denominator until a dedicated
  invalid-repository parity row exists.
- `"unsupported reftable ref value type"` in
  `crates/zmin-git-core/src/refs.rs` is a corrupt/internal reftable parse
  guard. It is documented and intentionally kept outside the Git `2.47.1`
  exact behavior denominator until a dedicated invalid-repository parity row
  exists.
