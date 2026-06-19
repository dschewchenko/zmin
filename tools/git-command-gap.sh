#!/usr/bin/env bash
set -euo pipefail

baseline="${ZMIN_GIT_BASELINE:-v2.32.0}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
zmin_bin="${ZMIN_BIN:-}"

if [[ -z "$zmin_bin" ]]; then
  rustup run stable cargo build --manifest-path "$repo_root/Cargo.toml" --release -p zmin-cli --bin zmin >/dev/null
  zmin_bin="$repo_root/target/release/zmin"
elif [[ "$zmin_bin" != /* ]]; then
  zmin_bin="$(cd "$repo_root" && pwd)/$zmin_bin"
fi

if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
  if [[ ! -x "$zmin_bin" && -x "${zmin_bin}.exe" ]]; then
    zmin_bin="${zmin_bin}.exe"
  fi
else
  if [[ ! -x "$zmin_bin" && -x "${zmin_bin}.exe" ]]; then
    zmin_bin="${zmin_bin}.exe"
  fi
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

command_list="$tmp_dir/command-list.txt"
if [[ -n "${ZMIN_GIT_COMMAND_LIST:-}" ]]; then
  cp "$ZMIN_GIT_COMMAND_LIST" "$command_list"
else
  curl -fsSL "https://raw.githubusercontent.com/git/git/${baseline}/command-list.txt" -o "$command_list"
fi

baseline_commands="$tmp_dir/baseline.tsv"
implemented_commands="$tmp_dir/implemented.txt"
implemented_tsv="$tmp_dir/implemented.tsv"
missing="$tmp_dir/missing.tsv"
covered="$tmp_dir/covered.tsv"

awk '
  $1 ~ /^git-/ {
    command = $1
    sub(/^git-/, "", command)
    print command "\t" $2
  }
' "$command_list" | sort -u >"$baseline_commands"

"$zmin_bin" --help \
  | awk '/^  [a-z0-9][a-z0-9-]+[[:space:]]/ { print $1 }' \
  | sort -u >"$implemented_commands"

awk 'NR == FNR { implemented[$1] = 1; next } ($1 in implemented) { print $0 }' \
  "$implemented_commands" "$baseline_commands" >"$covered"
awk 'NR == FNR { implemented[$1] = 1; next } !($1 in implemented) { print $0 }' \
  "$implemented_commands" "$baseline_commands" >"$missing"
awk '{ print $1 "\timplemented" }' "$implemented_commands" >"$implemented_tsv"

baseline_count="$(wc -l <"$baseline_commands" | tr -d ' ')"
implemented_baseline_count="$(wc -l <"$covered" | tr -d ' ')"
missing_count="$(wc -l <"$missing" | tr -d ' ')"
extra_count="$(comm -23 "$implemented_commands" <(cut -f1 "$baseline_commands" | sort -u) | wc -l | tr -d ' ')"
command_baseline_count="$(awk '$1 != "help" { count++ } END { print count + 0 }' "$baseline_commands")"
implemented_command_baseline_count="$(awk '$1 != "help" { count++ } END { print count + 0 }' "$covered")"
missing_command_baseline_count="$(awk '$1 != "help" { count++ } END { print count + 0 }' "$missing")"
command_baseline_percent="$(
  awk -v implemented="$implemented_command_baseline_count" -v total="$command_baseline_count" \
    'BEGIN { if (total == 0) print "0.0"; else printf "%.1f", implemented * 100 / total }'
)"

printf 'Git command gap against %s\n' "$baseline"
printf 'command_baseline=%s\n' "$command_baseline_count"
printf 'implemented_command_baseline=%s\n' "$implemented_command_baseline_count"
printf 'missing_command_baseline=%s\n' "$missing_command_baseline_count"
printf 'command_baseline_percent=%s\n' "$command_baseline_percent"
printf 'extra_commands=%s\n' "$extra_count"
printf 'raw_upstream_commands_including_help=%s\n' "$baseline_count"
printf 'raw_implemented_upstream_commands_including_help=%s\n' "$implemented_baseline_count"
printf 'raw_missing_upstream_commands_including_help=%s\n' "$missing_count"
printf '\nImplemented baseline commands by category:\n'
awk '{ count[$2]++ } END { for (category in count) print category, count[category] }' "$covered" | sort
printf '\nMissing baseline commands by category:\n'
awk '{ count[$2]++ } END { for (category in count) print category, count[category] }' "$missing" | sort
printf '\nMissing commands:\n'
column -t -s $'\t' "$missing"

if [[ "${ZMIN_GIT_GAP_STRICT:-0}" == "1" && "$missing_command_baseline_count" != "0" ]]; then
  exit 1
fi
