#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-upstream-compat-audit.sh [family-summary|legacy-audit]

Audits the pinned upstream Git shell-suite scope without vendoring the upstream
t/ tree into this repository.

Modes:
  family-summary  print per-family top-level test counts (tNNxx groups)
  legacy-audit    verify explicit legacy excludes against the upstream tree and
                  show nearby t9x families that remain in scope

Environment:
  ZMIN_UPSTREAM_GIT_TAG         Upstream Git tag. Default: v2.55.0.
  ZMIN_UPSTREAM_GIT_CACHE       Cache dir for upstream Git source/build.
  ZMIN_UPSTREAM_LEGACY_EXCLUDES Exclude TSV. Default:
                                tools/git-upstream-compat-tests-legacy-excludes.tsv
EOF
}

mode="${1:-}"
case "$mode" in
  family-summary|legacy-audit) ;;
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

if [[ ! -d "$source_dir/t" ]]; then
  echo "missing upstream Git source tree: $source_dir" >&2
  echo "run tools/git-upstream-compat-suite.sh once or populate ZMIN_UPSTREAM_GIT_CACHE first" >&2
  exit 2
fi

if [[ ! -f "$legacy_excludes" ]]; then
  echo "missing legacy exclude manifest: $legacy_excludes" >&2
  exit 2
fi

family_summary() {
  printf 'family\tcount\n'
  find "$source_dir/t" -maxdepth 1 -type f -name 't[0-9][0-9][0-9][0-9]-*.sh' -print |
    sed 's#^.*/##' |
    LC_ALL=C sort |
    cut -c1-3 |
    uniq -c |
    awk '{printf "%sxx\t%s\n", $2, $1}'
}

legacy_audit() {
  printf 'family\tprefix\texpected_count\tactual_count\tstatus\treason\n'
  awk -F '\t' '
    NR == 1 || /^#/ || NF < 5 { next }
    { print $1 "\t" $2 "\t" $3 "\t" $4 }
  ' "$legacy_excludes" |
    while IFS=$'\t' read -r family prefix expected_count reason; do
      actual_count="$(
        find "$source_dir/t" -maxdepth 1 -type f -name "${prefix}*.sh" -print |
          sed '/^$/d' |
          wc -l |
          tr -d ' '
      )"
      status="match"
      if [[ "$actual_count" != "$expected_count" ]]; then
        status="count-mismatch"
      fi
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$family" \
        "$prefix" \
        "$expected_count" \
        "$actual_count" \
        "$status" \
        "$reason"
    done

  printf '\n'
  printf 'in_scope_family\tcount\texample\tdescription\n'
  find "$source_dir/t" -maxdepth 1 -type f \( \
      -name 't90[0-9][0-9]-*.sh' -o \
      -name 't92[0-9][0-9]-*.sh' -o \
      -name 't93[0-9][0-9]-*.sh' -o \
      -name 't97[0-9][0-9]-*.sh' -o \
      -name 't99[0-9][0-9]-*.sh' \
    \) -print |
    sed 's#^.*/##' |
    LC_ALL=C sort |
    while IFS= read -r test_name; do
      family="${test_name:0:3}xx"
      description="$(
        sed -n '1,14p' "$source_dir/t/$test_name" |
          awk -F"'" '/^test_description=/{print $2; exit}'
      )"
      printf '%s\t%s\t%s\t%s\n' "$family" "1" "$test_name" "$description"
    done |
    awk -F '\t' '
      {
        counts[$1]++
        if (!($1 in example)) {
          example[$1] = $3
          description[$1] = $4
        }
      }
      END {
        for (family in counts) {
          printf "%s\t%s\t%s\t%s\n", family, counts[family], example[family], description[family]
        }
      }
    ' |
    LC_ALL=C sort
}

case "$mode" in
  family-summary)
    family_summary
    ;;
  legacy-audit)
    legacy_audit
    ;;
esac
