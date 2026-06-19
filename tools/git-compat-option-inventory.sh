#!/usr/bin/env bash
set -euo pipefail

baseline="${ZMIN_GIT_BASELINE:-v2.47.1}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cache_dir="${ZMIN_GIT_DOC_CACHE:-$repo_root/target/git-doc-cache/$baseline}"
command_list="${ZMIN_GIT_COMMAND_LIST:-$cache_dir/command-list.txt}"

mkdir -p "$cache_dir"

if [[ ! -f "$command_list" ]]; then
  curl -fsSL "https://raw.githubusercontent.com/git/git/${baseline}/command-list.txt" \
    -o "$command_list"
fi

printf 'command\toption\tdoc\n'

awk '$1 ~ /^git-/ { command = $1; sub(/^git-/, "", command); print command }' "$command_list" |
  sort -u |
  while IFS= read -r command; do
    doc="$cache_dir/git-$command.txt"
    if [[ ! -f "$doc" ]]; then
      if ! curl -fsSL "https://raw.githubusercontent.com/git/git/${baseline}/Documentation/git-${command}.txt" \
        -o "$doc"; then
        rm -f "$doc"
        continue
      fi
    fi

    awk -v command="$command" -v doc="git-$command.txt" '
      {
        line = $0
        gsub(/`/, "", line)
        gsub(/\[/, " ", line)
        gsub(/\]/, " ", line)
        gsub(/,/, " ", line)
        gsub(/\(/, " ", line)
        gsub(/\)/, " ", line)
        gsub(/::/, " ", line)
        n = split(line, parts, /[[:space:]]+/)
        for (i = 1; i <= n; i++) {
          token = parts[i]
          sub(/=.*$/, "", token)
          sub(/[;:.]$/, "", token)
          if (token ~ /^--[A-Za-z0-9][A-Za-z0-9-]*$/ || token ~ /^-[A-Za-z0-9?]$/) {
            print command "\t" token "\t" doc
          }
        }
      }
    ' "$doc"
  done |
  sort -u
