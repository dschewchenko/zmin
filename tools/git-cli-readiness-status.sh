#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-cli-readiness-status.sh [--require-complete]

Print command-entrypoint and catalog status.

This command does not claim drop-in compatibility. That requires the complete
upstream and differential oracle suites in addition to the catalog checks.
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

upstream_cache="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
current_source="$upstream_cache/git-v2.55.0"
[[ -d "$current_source" ]] || {
  echo "validated Git v2.55.0 source is missing: $current_source" >&2
  exit 1
}
current_source="$(cd "$current_source" && pwd -P)"
[[ "$(basename "$current_source")" == "git-v2.55.0" ]] || {
  echo "validated Git source basename is not git-v2.55.0: $current_source" >&2
  exit 1
}
[[ -d "$current_source/Documentation" ]] || {
  echo "validated Git Documentation directory is missing: $current_source/Documentation" >&2
  exit 1
}
current_command_list="$current_source/command-list.txt"
[[ -f "$current_command_list" && ! -L "$current_command_list" ]] || {
  echo "validated Git command-list.txt is missing: $current_command_list" >&2
  exit 1
}
archive_sha="$(awk -F '\t' '$1 == "upstream_archive_sha256" { print $2 }' "$repo_root/tools/git-upstream-compat-contract.tsv")"
[[ "$archive_sha" =~ ^[0-9a-f]{64}$ ]] || {
  echo "validated Git archive identity is missing from the current contract" >&2
  exit 1
}
marker="$current_source/.zmin-pristine-source.sha256"
[[ -f "$marker" ]] || {
  echo "validated Git source identity marker is missing: $marker" >&2
  exit 1
}
actual_archive_sha="$(tr -d '[:space:]' < "$marker")"
[[ "$actual_archive_sha" == "$archive_sha" ]] || {
  echo "validated Git source identity mismatch: expected $archive_sha, got ${actual_archive_sha:-<empty>}" >&2
  exit 1
}
export ZMIN_GIT_BASELINE=v2.55.0
export ZMIN_GIT_DOC_CACHE="$current_source"
export ZMIN_GIT_COMMAND_LIST="$current_command_list"
export ZMIN_GIT_SOURCE_ARCHIVE_SHA256="$archive_sha"

resolve_cargo_target_dir() {
  cargo metadata --no-deps --format-version 1 |
    python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])'
}

if [[ -n "${ZMIN_BIN:-}" ]]; then
  zmin_bin="$ZMIN_BIN"
else
  target_dir="$(resolve_cargo_target_dir)"
  cargo build -p zmin-cli --bin zmin --profile compat --quiet
  zmin_bin="$target_dir/compat/zmin"
fi
if [[ ! -x "$zmin_bin" ]]; then
  echo "Zmin binary is not executable: $zmin_bin" >&2
  exit 1
fi

stock_git="${ZMIN_STOCK_GIT:-${GIT_BIN:-}}"
if [[ -z "$stock_git" ]]; then
  for candidate in /usr/bin/git /bin/git; do
    if [[ -x "$candidate" ]] && ! "$candidate" --version | grep -qi 'zmin'; then
      stock_git="$candidate"
      break
    fi
  done
fi

if [[ -z "$stock_git" ]]; then
  stock_git="$(command -v git || true)"
fi

if [[ -z "$stock_git" || ! -x "$stock_git" ]]; then
  echo "stock Git binary is not executable: ${stock_git:-<empty>}" >&2
  exit 1
fi

if "$stock_git" --version | grep -qi 'zmin'; then
  echo "stock Git binary resolved to Zmin shim: $stock_git" >&2
  echo "set ZMIN_STOCK_GIT to a stock Git binary" >&2
  exit 1
fi

report="$(mktemp)"
inventory="$(mktemp)"
matrix_summary="$(mktemp)"
trap 'rm -f "$report" "$inventory" "$matrix_summary"' EXIT

"$zmin_bin" compat --profile v2-47 --format text >"$report"
ZMIN_BIN="$zmin_bin" ZMIN_STOCK_GIT="$stock_git" tools/run-current-git-command-inventory.sh >"$inventory"
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
behavior_rows_classified="$(awk -F'\t' '$1 == "behavior_rows_classified" { print $2 }' "$matrix_summary")"
behavior_rows_open="$(awk -F'\t' '$1 == "behavior_rows_open" { print $2 }' "$matrix_summary")"
invalid_input_rows="$(awk -F'\t' '$1 == "invalid_input_rows" { print $2 }' "$matrix_summary")"

printf 'Git CLI readiness status\n'
printf 'profile=v2-47\n'
printf 'zmin_bin=%s\n' "$zmin_bin"
printf 'zmin_version=%s\n' "$("$zmin_bin" --version)"
printf 'zmin_sha256=%s\n' "$(shasum -a 256 "$zmin_bin" | awk '{ print $1 }')"
printf 'stock_git_bin=%s\n' "$stock_git"
printf 'stock_git_version=%s\n' "$("$stock_git" --version)"
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
printf 'behavior_rows_classified=%s/%s\n' "$behavior_rows_classified" "$behavior_rows_written"
printf 'behavior_rows_open=%s/%s\n' "$behavior_rows_open" "$behavior_rows_written"
printf 'invalid_input_rows=%s/%s\n' "$invalid_input_rows" "$behavior_rows_written"

if [[ "$not_ready_count" == "0" &&
      "$missing_baseline_count" == "0" &&
      "$unexpected_missing_count" == "0" &&
      "$complete_command_matrices" == "$total_command_matrices" &&
      "$complete_doc_option_pairs" == "$total_doc_option_pairs" &&
      "$behavior_rows_open" == "0" ]]; then
  printf 'catalog_status=complete\n'
  printf 'drop_in_compatibility=unverified\n'
  exit 0
fi

printf 'catalog_status=incomplete\n'
printf 'drop_in_compatibility=unverified\n'
if [[ "$require_complete" == true ]]; then
  exit 1
fi
