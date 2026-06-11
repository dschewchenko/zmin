#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
skron_bin="${SKRON_BIN:-}"
allowed_omissions="${SKRON_CURRENT_GIT_ALLOWED_OMISSIONS:-}"

if [[ -z "$skron_bin" ]]; then
  cargo build -p skron-cli --bin skron --no-default-features --quiet
  skron_bin="$repo_root/target/debug/skron"
elif [[ "$skron_bin" != /* ]]; then
  skron_bin="$repo_root/$skron_bin"
fi

if [[ ! -x "$skron_bin" ]]; then
  echo "skron binary is not executable: $skron_bin" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

current_git="$tmp_dir/current-git.txt"
skron_commands="$tmp_dir/skron.txt"
allowed="$tmp_dir/allowed.txt"
missing="$tmp_dir/missing.txt"
unexpected_missing="$tmp_dir/unexpected-missing.txt"
allowed_missing="$tmp_dir/allowed-missing.txt"
extra="$tmp_dir/extra.txt"

git help -a | awk '
  /^Main Porcelain Commands$/ ||
  /^Ancillary Commands/ ||
  /^Interacting with Others$/ ||
  /^Low-level Commands/ { in_commands = 1; next }

  /^User-facing / ||
  /^Developer-facing / ||
  /^External commands$/ { in_commands = 0; next }

  in_commands && $1 ~ /^[a-z0-9][a-z0-9-]+$/ {
    print "git-" $1
  }
' | sort -u >"$current_git"

"$skron_bin" compat --profile modern --format text \
  | awk -F: '/^git-[a-z0-9][a-z0-9-]*:/ { print $1 }' \
  | sort -u >"$skron_commands"

for command in $allowed_omissions; do
  printf '%s\n' "$command"
done | sort -u >"$allowed"

comm -23 "$current_git" "$skron_commands" >"$missing"
comm -12 "$missing" "$allowed" >"$allowed_missing"
comm -23 "$missing" "$allowed" >"$unexpected_missing"
comm -13 "$current_git" "$skron_commands" >"$extra"

current_count="$(wc -l <"$current_git" | tr -d ' ')"
skron_count="$(wc -l <"$skron_commands" | tr -d ' ')"
missing_count="$(wc -l <"$missing" | tr -d ' ')"
allowed_missing_count="$(wc -l <"$allowed_missing" | tr -d ' ')"
unexpected_missing_count="$(wc -l <"$unexpected_missing" | tr -d ' ')"
extra_count="$(wc -l <"$extra" | tr -d ' ')"

printf 'Current Git command inventory\n'
printf 'git_version=%s\n' "$(git --version)"
printf 'current_git_commands=%s\n' "$current_count"
printf 'skron_modern_commands=%s\n' "$skron_count"
printf 'missing_current_git_commands=%s\n' "$missing_count"
printf 'allowed_omitted_current_git_commands=%s\n' "$allowed_missing_count"
printf 'unexpected_missing_current_git_commands=%s\n' "$unexpected_missing_count"
printf 'extra_skron_commands=%s\n' "$extra_count"

printf '\nAllowed omitted current Git commands:\n'
if [[ -s "$allowed_missing" ]]; then
  cat "$allowed_missing"
else
  printf '(none)\n'
fi

printf '\nUnexpected missing current Git commands:\n'
if [[ -s "$unexpected_missing" ]]; then
  cat "$unexpected_missing"
else
  printf '(none)\n'
fi

printf '\nExtra Skron commands outside current Git help -a command sections:\n'
if [[ -s "$extra" ]]; then
  cat "$extra"
else
  printf '(none)\n'
fi

if [[ "$unexpected_missing_count" != "0" ]]; then
  exit 1
fi
