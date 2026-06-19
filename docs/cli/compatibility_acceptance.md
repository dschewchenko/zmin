# Git CLI Compatibility Acceptance

Scope: stock Git compatibility on macOS and Linux for the current tree.

## Acceptance line

The repository is considered complete for its current scope when all of the following are true:

- `tools/git-cli-readiness-status.sh --require-complete` exits `0`
- `cargo test -p zmin-cli --all-targets` passes
- `cargo test -p zmin-git-core --all-targets` passes
- current Git command inventory reports zero missing command entry points for
  the selected baseline
- every Git `v2.47.1` command has an audited option/value behavior matrix
- the live `v2-47` compatibility report keeps `151` baseline commands
  matched, `0` missing commands, and additive Zmin commands documented as
  extra surface rather than baseline failures
- every matrix row is `closed`, `out-of-scope` with a documented reason, or
  `invalid-input` matching stock Git diagnostics

## Evidence

The compatibility proof is built from three layers:

1. Command entry-point inventory parity against tracked Git baselines.
2. Option/value inventory from Git documentation and stock Git help.
3. Behavior matrix rows checked against stock Git.
4. Scenario coverage in `crates/zmin-cli/tests/`.
5. Repository-state handoff checks proving that stock Git and Zmin can operate on the same repository state without rewriting structure.

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

For a seed option inventory from the Git documentation baseline:

```bash
tools/git-compat-option-inventory.sh
```
