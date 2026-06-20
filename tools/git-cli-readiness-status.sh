#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-cli-readiness-status.sh [--require-complete]

Print command-entrypoint readiness and full matrix compatibility status.
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

zmin_bin="${ZMIN_BIN:-$repo_root/target/debug/zmin}"
if [[ ! -x "$zmin_bin" ]]; then
  cargo build -p zmin-cli --bin zmin --quiet
fi

report="$(mktemp)"
inventory="$(mktemp)"
matrix_summary="$(mktemp)"
trap 'rm -f "$report" "$inventory" "$matrix_summary"' EXIT

"$zmin_bin" compat --profile v2-47 --format text >"$report"
ZMIN_BIN="$zmin_bin" tools/run-current-git-command-inventory.sh >"$inventory"
tools/git-compat-command-summary.sh --tsv >"$matrix_summary"

ready_line="$(grep -E '^Ready commands: [0-9]+ \(explicitly not ready: [0-9]+\)$' "$report")"
command_line="$(grep -E '^Commands: expected [0-9]+, implemented [0-9]+, matching baseline [0-9]+, missing [0-9]+, extra [0-9]+$' "$report")"
ready_count="$(printf '%s\n' "$ready_line" | sed -E 's/^Ready commands: ([0-9]+) \(explicitly not ready: ([0-9]+)\)$/\1/')"
not_ready_count="$(printf '%s\n' "$ready_line" | sed -E 's/^Ready commands: ([0-9]+) \(explicitly not ready: ([0-9]+)\)$/\2/')"
missing_baseline_count="$(printf '%s\n' "$command_line" | sed -E 's/^Commands: expected [0-9]+, implemented [0-9]+, matching baseline [0-9]+, missing ([0-9]+), extra [0-9]+$/\1/')"
unexpected_missing_count="$(awk -F= '/^unexpected_missing_current_git_commands=/ { print $2 }' "$inventory")"
complete_command_matrices="$(awk -F'\t' '$1 == "complete_command_matrices" { print $2 }' "$matrix_summary")"
total_command_matrices="$(awk -F'\t' '$1 == "complete_command_matrices" { print $3 }' "$matrix_summary")"
complete_doc_option_pairs="$(awk -F'\t' '$1 == "complete_doc_option_pairs" { print $2 }' "$matrix_summary")"
total_doc_option_pairs="$(awk -F'\t' '$1 == "complete_doc_option_pairs" { print $3 }' "$matrix_summary")"
commands_with_matrix_rows="$(awk -F'\t' '$1 == "commands_with_matrix_rows" { print $2 }' "$matrix_summary")"
total_commands_with_matrix_rows="$(awk -F'\t' '$1 == "commands_with_matrix_rows" { print $3 }' "$matrix_summary")"
represented_doc_option_pairs="$(awk -F'\t' '$1 == "doc_option_pairs_represented_by_rows" { print $2 }' "$matrix_summary")"
total_represented_doc_option_pairs="$(awk -F'\t' '$1 == "doc_option_pairs_represented_by_rows" { print $3 }' "$matrix_summary")"
behavior_rows_written="$(awk -F'\t' '$1 == "behavior_rows_written" { print $2 }' "$matrix_summary")"
written_rows_matching_stock_git="$(awk -F'\t' '$1 == "written_rows_matching_stock_git" { print $2 }' "$matrix_summary")"
behavior_rows_open="$(awk -F'\t' '$1 == "behavior_rows_open" { print $2 }' "$matrix_summary")"
invalid_input_rows="$(awk -F'\t' '$1 == "invalid_input_rows" { print $2 }' "$matrix_summary")"

printf 'Git CLI readiness status\n'
printf 'profile=v2-47\n'
printf 'command_entrypoints_ready=%s\n' "$ready_count"
printf 'explicit_not_ready=%s\n' "$not_ready_count"
printf 'baseline_missing=%s\n' "$missing_baseline_count"
printf 'unexpected_missing_current_git_commands=%s\n' "$unexpected_missing_count"
printf 'complete_command_matrices=%s/%s\n' "$complete_command_matrices" "$total_command_matrices"
printf 'complete_doc_option_pairs=%s/%s\n' "$complete_doc_option_pairs" "$total_doc_option_pairs"
printf 'commands_with_matrix_rows=%s/%s\n' "$commands_with_matrix_rows" "$total_commands_with_matrix_rows"
printf 'doc_option_pairs_represented_by_rows=%s/%s\n' "$represented_doc_option_pairs" "$total_represented_doc_option_pairs"
printf 'behavior_rows_written=%s\n' "$behavior_rows_written"
printf 'written_rows_matching_stock_git=%s/%s\n' "$written_rows_matching_stock_git" "$behavior_rows_written"
printf 'behavior_rows_open=%s/%s\n' "$behavior_rows_open" "$behavior_rows_written"
printf 'invalid_input_rows=%s/%s\n' "$invalid_input_rows" "$behavior_rows_written"

if [[ "$not_ready_count" == "0" &&
      "$missing_baseline_count" == "0" &&
      "$unexpected_missing_count" == "0" &&
      "$complete_command_matrices" == "$total_command_matrices" &&
      "$complete_doc_option_pairs" == "$total_doc_option_pairs" &&
      "$behavior_rows_open" == "0" ]]; then
  printf 'status=complete\n'
  exit 0
fi

printf 'status=matrix-incomplete\n'
if [[ "$require_complete" == true ]]; then
  exit 1
fi
