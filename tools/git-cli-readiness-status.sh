#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-cli-readiness-status.sh [--require-complete]

Print the current Git CLI compatibility status for this tree.
EOF
}

require_complete=false
case "${1:-}" in
  "")
    ;;
  --require-complete|--require-macos-linux)
    require_complete=true
    ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

skron_bin="${SKRON_BIN:-$repo_root/target/debug/skron}"
if [[ ! -x "$skron_bin" ]]; then
  cargo build -p skron-cli --bin skron --quiet
fi

report="$(mktemp)"
inventory="$(mktemp)"
trap 'rm -f "$report" "$inventory"' EXIT

"$skron_bin" compat --profile v2-47 --format text >"$report"
SKRON_BIN="$skron_bin" tools/run-current-git-command-inventory.sh >"$inventory"

ready_line="$(grep -E '^Ready commands: [0-9]+ \(explicitly not ready: [0-9]+\)$' "$report")"
command_line="$(grep -E '^Commands: expected [0-9]+, implemented [0-9]+, matching baseline [0-9]+, missing [0-9]+, extra [0-9]+$' "$report")"
ready_count="$(printf '%s\n' "$ready_line" | sed -E 's/^Ready commands: ([0-9]+) \(explicitly not ready: ([0-9]+)\)$/\1/')"
not_ready_count="$(printf '%s\n' "$ready_line" | sed -E 's/^Ready commands: ([0-9]+) \(explicitly not ready: ([0-9]+)\)$/\2/')"
missing_baseline_count="$(printf '%s\n' "$command_line" | sed -E 's/^Commands: expected [0-9]+, implemented [0-9]+, matching baseline [0-9]+, missing ([0-9]+), extra [0-9]+$/\1/')"
unexpected_missing_count="$(awk -F= '/^unexpected_missing_current_git_commands=/ { print $2 }' "$inventory")"

printf 'Git CLI readiness status\n'
printf 'profile=v2-47\n'
printf 'ready_commands=%s\n' "$ready_count"
printf 'explicit_not_ready=%s\n' "$not_ready_count"
printf 'baseline_missing=%s\n' "$missing_baseline_count"
printf 'unexpected_missing_current_git_commands=%s\n' "$unexpected_missing_count"

if [[ "$not_ready_count" == "0" && "$missing_baseline_count" == "0" && "$unexpected_missing_count" == "0" ]]; then
  printf 'status=complete\n'
  exit 0
fi

printf 'status=blocked\n'
if [[ "$require_complete" == true ]]; then
  exit 1
fi
