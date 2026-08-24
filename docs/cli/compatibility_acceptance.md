# Git CLI Compatibility Acceptance

Scope: the frozen Git `v2.55.0` current/nondeprecated contract in
[`docs/git/upstream_compatibility_baseline.md`](../git/upstream_compatibility_baseline.md).
The pinned upstream commit is
`e9019fcafe0040228b8631c30f97ae1adb61bcdc`. Of `1046` top-level upstream test
files, the authoritative `all-nondeprecated` denominator is `1045`: the sole
whole-file exclusion is `t5323-pack-redundant.sh` (`git pack-redundant`). The
external-but-current groups `git-svn` (`t91*`), `git-cvsserver` (`t94*`),
`gitweb` (`t95*`), `cvsimport` (`t96*`) and `git-p4` (`t9800`-`t9836`) remain
included. This document does not define a second scope.

Deprecated or removal-marked assertions inside retained tests—including
symlink refs/`core.preferSymlinkRefs`, `show-ref --heads`,
`ls-remote --heads`/`-h`, `name-rev --stdin`, `whatchanged`,
`core.commentChar=auto`, and `.git/info/grafts`—are not additional exclusions.
Legacy `.git/remotes`/`.git/branches` formats and dynamic deprecated-builtin
alias cases remain retained pending an explicit product decision.

## Acceptance line

The repository is considered complete for its current scope when all of the following are true:

- `tools/git-cli-readiness-status.sh --require-complete` exits `0`
- `tools/git-upstream-compat-contract-gate.sh check` exits `0`
- `cargo test -p zmin-cli --all-targets` passes
- `cargo test -p zmin-git-core --all-targets` passes
- the frozen upstream `all-nondeprecated` suite passes on the required
  supported platforms
- the stock-Git differential suite and required platform evidence pass for the
  same behavior scope
- every matrix row is `closed`, `out-of-scope` with a documented reason, or
  `invalid-input` matching stock Git diagnostics

The contract gate is a scope prerequisite, not a compatibility claim. A suite
run is authoritative upstream evidence only when its `run-metadata.tsv` passes
`tools/git-upstream-compat-contract-gate.sh validate-run --require-authoritative`;
custom, bounded, failed or exploratory runs must remain labeled as such.

## Platform-specific implementation blocker

Fast-import pack and edge-pack publication is currently unsupported on
Windows: Zmin fails closed before the required handle-relative publication
path. This is an implementation blocker, not an exclusion from the current
Git denominator. `t9300-fast-import.sh` remains in `all-nondeprecated`, but
historical Windows selected-surface evidence cannot establish Windows t9300
readiness. Current authoritative platform and performance-runner limitations
are defined once in
[`docs/git/performance_evidence_contract.md`](../git/performance_evidence_contract.md).

## Evidence

The compatibility proof is built from these layers:

1. The frozen source archive and manifest contract check.
2. The pinned upstream Git shell suite.
3. Behavior matrix rows and stock-Git differential checks.
4. macOS, Linux and Windows platform evidence where behavior can differ.
5. Repository-state handoff checks proving that stock Git and Zmin can operate
   on the same repository state without rewriting structure.

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
