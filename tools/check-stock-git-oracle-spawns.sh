#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

direct_hazards="$(mktemp)"
parameter_hazards="$(mktemp)"
trap 'rm -f "$direct_hazards" "$parameter_hazards"' EXIT

rg -n 'Command::new\("git"\)' \
  crates/zmin-cli/tests \
  crates/zmin-cli/src/cli/commands/maintenance_impl.rs \
  >"$direct_hazards" || true

rg -n '(std::process::)?Command::new\(command\)' \
  crates/zmin-cli/tests \
  crates/zmin-cli/src/cli/commands/maintenance_impl.rs \
  | grep -v '^crates/zmin-cli/tests/git_transport_http_compat.rs:' \
  >"$parameter_hazards" || true

if [[ -s "$direct_hazards" || -s "$parameter_hazards" ]]; then
  echo "stock Git oracle spawn hazards found" >&2
  if [[ -s "$direct_hazards" ]]; then
    cat "$direct_hazards" >&2
  fi
  if [[ -s "$parameter_hazards" ]]; then
    cat "$parameter_hazards" >&2
  fi
  echo "use common::stock_git_bin() or common::test_command_program(command)" >&2
  exit 1
fi

printf 'stock_git_oracle_spawn_hazards=0\n'
