# Command Compatibility Audit

This file is the compact public summary of the current Git command surface.

## Baselines

- Git `v2.32.0`: `145/145` tracked commands present
- Git `v2.47.1`: `150/150` tracked commands present

## Current state

- no missing commands for the tracked baselines
- compatibility report is generated from the live CLI schema, not from a hand-maintained list
- extra commands stay visible in the report as additive surface, not as baseline failures

## Commands to run

```bash
SKRON_GIT_GAP_STRICT=1 ./tools/git-command-gap.sh
SKRON_GIT_BASELINE=v2.47.1 ./tools/git-command-gap.sh
cargo test -p skron-cli --test compatibility_command
```
