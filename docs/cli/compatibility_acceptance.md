# Git CLI Compatibility Acceptance

Scope: stock Git compatibility on macOS and Linux for the current tree.

## Acceptance line

The repository is considered complete for its current scope when all of the following are true:

- `tools/git-cli-readiness-status.sh --require-complete` exits `0`
- `cargo test -p zmin-cli --all-targets` passes
- `cargo test -p zmin-git-core --all-targets` passes
- current Git command inventory reports zero missing commands for the selected baseline
- the live `v2-47` compatibility report keeps `151` baseline commands
  matched, `0` missing commands, and additive Zmin commands documented as
  extra surface rather than baseline failures

## Evidence

The compatibility proof is built from three layers:

1. Command inventory parity against tracked Git baselines.
2. Scenario coverage in `crates/zmin-cli/tests/`.
3. Repository-state handoff checks proving that stock Git and Zmin can operate on the same repository state without rewriting structure.

## Main commands

```bash
tools/git-cli-readiness-status.sh --require-complete
cargo test -p zmin-cli --all-targets
cargo test -p zmin-git-core --all-targets
```

For a quick command inventory refresh:

```bash
tools/run-current-git-command-inventory.sh
```
