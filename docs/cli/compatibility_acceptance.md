# Git CLI Compatibility Acceptance

Scope: stock Git compatibility on macOS and Linux for the current tree.

## Acceptance line

The repository is considered complete for its current scope when all of the following are true:

- `tools/git-cli-readiness-status.sh --require-complete` exits `0`
- `cargo test -p skron-cli --all-targets` passes
- `cargo test -p skron-git-core --all-targets` passes
- current Git command inventory reports zero missing commands for the selected baseline

## Evidence

The compatibility proof is built from three layers:

1. Command inventory parity against tracked Git baselines.
2. Scenario coverage in `crates/skron-cli/tests/`.
3. Repository-state handoff checks proving that stock Git and Skron can operate on the same repository state without rewriting structure.

## Main commands

```bash
tools/git-cli-readiness-status.sh --require-complete
cargo test -p skron-cli --all-targets
cargo test -p skron-git-core --all-targets
```

For a quick command inventory refresh:

```bash
tools/run-current-git-command-inventory.sh
```
