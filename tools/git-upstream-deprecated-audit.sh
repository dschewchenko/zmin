#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-upstream-deprecated-audit.sh [summary|audit]

Audits deprecated upstream Git shell-suite surfaces without vendoring the
upstream t/ tree into this repository.

Modes:
  summary  print machine-readable counts for fully excluded versus mixed
           deprecated shell-suite files
  audit    print the classified deprecated top-level upstream shell tests

Environment:
  ZMIN_UPSTREAM_GIT_TAG         Upstream Git tag. Default: v2.55.0.
  ZMIN_UPSTREAM_GIT_CACHE       Cache dir for upstream Git source/build.
  ZMIN_UPSTREAM_LEGACY_EXCLUDES Exclude TSV. Default:
                                tools/git-upstream-compat-tests-legacy-excludes.tsv
  ZMIN_UPSTREAM_FILE_EXCLUDES   Optional per-file exclude TSV. Default:
                                tools/git-upstream-compat-tests-file-excludes.tsv
EOF
}

mode="${1:-summary}"
case "$mode" in
  summary|audit) ;;
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
tag="${ZMIN_UPSTREAM_GIT_TAG:-v2.55.0}"
cache_root="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
source_dir="$cache_root/git-$tag"
legacy_excludes="${ZMIN_UPSTREAM_LEGACY_EXCLUDES:-$repo_root/tools/git-upstream-compat-tests-legacy-excludes.tsv}"
file_excludes="${ZMIN_UPSTREAM_FILE_EXCLUDES:-$repo_root/tools/git-upstream-compat-tests-file-excludes.tsv}"

if [[ ! -d "$source_dir/t" ]]; then
  echo "missing upstream Git source tree: $source_dir" >&2
  echo "run tools/git-upstream-compat-suite.sh once or populate ZMIN_UPSTREAM_GIT_CACHE first" >&2
  exit 2
fi

if [[ ! -f "$legacy_excludes" ]]; then
  echo "missing legacy exclude manifest: $legacy_excludes" >&2
  exit 2
fi

deprecated_tmp="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-deprecated.XXXXXX")"
patterns_tmp="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-deprecated-patterns.XXXXXX")"
matches_tmp="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-deprecated-matches.XXXXXX")"
trap 'rm -f "$deprecated_tmp" "$patterns_tmp" "$matches_tmp"' EXIT

cat >"$patterns_tmp" <<'EOF'
deprecated
scheduled for removal
will be removed in Git 3\.0
--i-still-use-this
WITH_BREAKING_CHANGES
EOF

is_excluded_family() {
  local test_name="$1"
  awk -F '\t' -v test_name="$test_name" '
    NR == 1 || /^#/ || NF < 2 { next }
    index(test_name, $2) == 1 { found = 1 }
    END { exit(found ? 0 : 1) }
  ' "$legacy_excludes"
}

is_excluded_file() {
  local test_name="$1"
  if [[ ! -f "$file_excludes" ]]; then
    return 1
  fi
  awk -F '\t' -v test_name="$test_name" '
    NR == 1 || /^#/ || NF < 1 { next }
    $1 == test_name { found = 1 }
    END { exit(found ? 0 : 1) }
  ' "$file_excludes"
}

family_reason() {
  local test_name="$1"
  awk -F '\t' -v test_name="$test_name" '
    NR == 1 || /^#/ || NF < 5 { next }
    index(test_name, $2) == 1 { print $4; exit }
  ' "$legacy_excludes"
}

file_reason() {
  local test_name="$1"
  if [[ ! -f "$file_excludes" ]]; then
    return 1
  fi
  awk -F '\t' -v test_name="$test_name" '
    NR == 1 || /^#/ || NF < 2 { next }
    $1 == test_name { print $2; exit }
  ' "$file_excludes"
}

printf 'classification\ttest\tdeprecated_markers\treason\n' >"$deprecated_tmp"
{
  rg -n -i -f "$patterns_tmp" "$source_dir/t" -g 't[0-9][0-9][0-9][0-9]-*.sh' || true
} |
  awk -F ':' '
    {
      file = $1
      sub(/^.*\//, "", file)
      counts[file]++
    }
    END {
      for (file in counts) {
        printf "%s\t%d\n", file, counts[file]
      }
    }
  ' |
  LC_ALL=C sort >"$matches_tmp"

while IFS=$'\t' read -r test_name markers; do
  [[ -z "$test_name" ]] && continue

  classification="mixed-in-scope"
  reason="contains deprecated assertions inside a still-supported upstream shell test"
  if is_excluded_family "$test_name"; then
    classification="fully-excluded-family"
    reason="$(family_reason "$test_name")"
  elif is_excluded_file "$test_name"; then
    classification="fully-excluded-file"
    reason="$(file_reason "$test_name")"
  fi

  printf '%s\t%s\t%s\t%s\n' \
    "$classification" \
    "$test_name" \
    "$markers" \
    "$reason" >>"$deprecated_tmp"
done <"$matches_tmp"

if [[ "$mode" == "summary" ]]; then
  printf 'bucket\tcount\treason\n'
  awk -F '\t' '
    NR == 1 { next }
    {
      counts[$1]++
    }
    END {
      printf "deprecated_top_level_shell_files\t%d\ttop-level upstream shell tests with explicit deprecated/removal markers\n", counts["mixed-in-scope"] + counts["fully-excluded-family"] + counts["fully-excluded-file"]
      printf "deprecated_fully_excluded_family_files\t%d\texplicit legacy/deprecated/external family excludes that also carry deprecated markers\n", counts["fully-excluded-family"]
      printf "deprecated_fully_excluded_file_rows\t%d\toptional per-file excludes that also carry deprecated markers\n", counts["fully-excluded-file"]
      printf "deprecated_mixed_in_scope_files\t%d\tstill-supported upstream shell tests that keep deprecated assertions alongside live command coverage\n", counts["mixed-in-scope"]
    }
  ' "$deprecated_tmp"
  exit 0
fi

cat "$deprecated_tmp"
