#!/usr/bin/env bash
set -euo pipefail

base="${1:-9275ac4d}"
target="${2:-HEAD}"

printf 'commit\tsubject\tinsertions\tdeletions\tnet_matrix_lines\n'

git rev-list --reverse "${base}..${target}" | while IFS= read -r rev; do
  stats="$(
    git show --numstat --format= "$rev" -- 'docs/cli/matrices/*_v2_47.tsv' \
      | awk '
          {
            insertions += $1
            deletions += $2
          }
          END {
            printf "%d\t%d\t%d", insertions + 0, deletions + 0, insertions - deletions
          }
        '
  )"
  net="$(printf '%s' "$stats" | cut -f3)"
  if [ "$net" -ne 0 ]; then
    printf '%s\t%s\t%s\n' "$(git rev-parse --short "$rev")" "$(git log -1 --format=%s "$rev")" "$stats"
  fi
done
