#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets --all-features
cargo test -p skron-git-core --all-targets
cargo test -p skron-cli --all-targets
tools/git-cli-readiness-status.sh --require-complete
