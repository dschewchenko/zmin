# Git Compatibility Evidence Matrix

Date: 2026-05-16

## Baseline and command coverage

| Surface | Command | Result |
| --- | --- | --- |
| Command baseline (v2.32.0) | `SKRON_GIT_GAP_STRICT=1 ./tools/git-command-gap.sh` | `145/145`, `100.0%`, `0` gaps |
| Command baseline (v2.47.1) | `SKRON_GIT_BASELINE=v2.47.1 ./tools/git-command-gap.sh` | `150/150`, `100.0%`, `0` gaps |
| CLI compatibility suite | `cargo test -p skron-cli --all-targets` | `486/486` passing tests |
| Core primitive suite | `cargo test -p skron-git-core --all-targets` | `66/66` passing tests |

## Repository-state proof

The repository handoff proof lives primarily in:

- `crates/skron-cli/tests/git_repository_state_compat.rs`
- `crates/skron-cli/tests/git_worktree_state_compat.rs`
- `crates/skron-cli/tests/git_transport_local_compat.rs`

These checks cover:

- worktree state
- index state
- refs and reflogs
- loose objects and packfiles
- local clone, fetch, push, and pull handoff

## Provider smoke

Real-repository smoke and provider checks are driven by:

- `tools/git-provider-smoke.sh`
- `tools/git-real-repo-smoke.sh`

They are used as extra proof on top of the local compatibility suites, not as a substitute for them.
