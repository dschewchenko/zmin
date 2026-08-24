#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Never report catalog/readiness status before the frozen current-Git contract
# and pinned upstream source identity have passed their fail-closed gate.
tools/git-upstream-compat-contract-gate.sh check

cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets --all-features
cargo test -p zmin-git-core --all-targets
cargo test -p zmin-cli --all-targets
tools/git-cli-readiness-status.sh --require-complete
