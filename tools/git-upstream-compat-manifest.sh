#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-upstream-compat-manifest.sh [all-top-level|all-nondeprecated|full-core]

Generates upstream Git test manifests without vendoring the upstream t/ tree
into this repository.

Modes:
  all-top-level
              every top-level upstream tNNNN-*.sh shell test
  all-nondeprecated
              every top-level upstream tNNNN-*.sh shell test except the
              explicit per-file deprecated excludes
  full-core   every top-level upstream tNNNN-*.sh shell test except the
              explicitly excluded legacy/external families

Environment:
  ZMIN_UPSTREAM_GIT_TAG        Upstream Git tag. Default: v2.55.0.
  ZMIN_UPSTREAM_GIT_CACHE      Cache dir for upstream Git source/build.
  ZMIN_UPSTREAM_LEGACY_EXCLUDES
                               Exclude TSV. Default:
                               tools/git-upstream-compat-tests-legacy-excludes.tsv
  ZMIN_UPSTREAM_FILE_EXCLUDES  Optional per-file exclude TSV. Default:
                               tools/git-upstream-compat-tests-file-excludes.tsv
EOF
}

mode="${1:-}"
case "$mode" in
  all-top-level|all-nondeprecated|full-core) ;;
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
legacy_excludes="${ZMIN_UPSTREAM_LEGACY_EXCLUDES:-$repo_root/tools/git-upstream-compat-tests-legacy-excludes.tsv}"
file_excludes="${ZMIN_UPSTREAM_FILE_EXCLUDES:-$repo_root/tools/git-upstream-compat-tests-file-excludes.tsv}"
deprecated_tool="$repo_root/tools/git-upstream-deprecated-audit.sh"

if [[ ! -f "$legacy_excludes" ]]; then
  echo "missing legacy exclude manifest: $legacy_excludes" >&2
  exit 2
fi

legacy_pattern_file="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-legacy-patterns.XXXXXX")"
file_pattern_file="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-file-patterns.XXXXXX")"
deprecated_pattern_file="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-deprecated-patterns.XXXXXX")"
direct_tests_tmp="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-direct-tests.XXXXXX")"
trap 'rm -f "$legacy_pattern_file" "$file_pattern_file" "$deprecated_pattern_file" "$direct_tests_tmp"' EXIT

"$deprecated_tool" source-files >"$direct_tests_tmp"

awk -F '\t' '
  /^#/ || NF < 2 { next }
  { print "^" $2 }
' "$legacy_excludes" >"$legacy_pattern_file"

if [[ -f "$file_excludes" ]]; then
  awk -F '\t' '
    /^#/ || NF < 1 || $1 == "" { next }
    { print "^" $1 "$" }
  ' "$file_excludes" >"$file_pattern_file"
else
  : >"$file_pattern_file"
fi

if [[ "$mode" == "all-nondeprecated" ]]; then
  "$deprecated_tool" audit |
    awk -F '\t' '
      NR == 1 { next }
      $1 == "fully-excluded-family" || $1 == "fully-excluded-file" {
        print "^" $2 "$"
      }
    ' >"$deprecated_pattern_file"
else
  : >"$deprecated_pattern_file"
fi

echo '# mode<TAB>test<TAB>reason'
while IFS= read -r test_name; do
    case "$mode" in
      all-top-level)
        printf 'all-top-level\t%s\t%s\n' \
          "$test_name" \
          'complete upstream top-level shell suite'
        ;;
      all-nondeprecated)
        if [[ -s "$deprecated_pattern_file" ]] && grep -Eq -f "$deprecated_pattern_file" <<<"$test_name"; then
          continue
        fi
        if [[ -s "$file_pattern_file" ]] && grep -Eq -f "$file_pattern_file" <<<"$test_name"; then
          continue
        fi
        printf 'all-nondeprecated\t%s\t%s\n' \
          "$test_name" \
          'complete upstream top-level shell suite minus explicit whole-file deprecated excludes'
        ;;
      full-core)
        if [[ -s "$legacy_pattern_file" ]] && grep -Eq -f "$legacy_pattern_file" <<<"$test_name"; then
          continue
        fi
        if [[ -s "$file_pattern_file" ]] && grep -Eq -f "$file_pattern_file" <<<"$test_name"; then
          continue
        fi
        printf 'exhaustive\t%s\t%s\n' \
          "$test_name" \
          'full upstream shell suite minus explicit legacy/external excludes'
        ;;
    esac
done <"$direct_tests_tmp"
