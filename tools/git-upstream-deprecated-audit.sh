#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-upstream-deprecated-audit.sh [summary|audit|source-files]

Audits deprecated upstream Git shell-suite surfaces without vendoring the
upstream t/ tree into this repository.

Modes:
  summary  print machine-readable counts for deprecated/removal markers and
           distinct WITH_BREAKING_CHANGES markers
  audit    print classified top-level upstream shell tests and marker counts
  source-files
           print the authenticated direct t/tNNNN-*.sh file list

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
  summary|audit|source-files) ;;
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

contract_file="$repo_root/tools/git-upstream-compat-contract.tsv"
contract_value() {
  local key="$1"
  awk -F '\t' -v key="$key" '$1 == key { print $2; exit }' "$contract_file"
}

validate_pinned_source() {
  local contract_tag contract_archive_sha archive archive_sha marker marker_sha
  local canonical_cache canonical_source expected_total actual_total invalid_surface
  contract_tag="$(contract_value upstream_git_tag)"
  contract_archive_sha="$(contract_value upstream_archive_sha256)"
  expected_total="$(contract_value upstream_top_level_shell_tests)"
  [[ "$tag" == "$contract_tag" ]] || {
    echo "deprecated audit requires pinned upstream tag $contract_tag" >&2
    return 1
  }
  [[ "$cache_root" == /* && -d "$cache_root" && ! -L "$cache_root" ]] || {
    echo "upstream cache root must be an existing absolute non-symlink directory" >&2
    return 1
  }
  canonical_cache="$(cd -P "$cache_root" && pwd -P)" || return 1
  [[ "$canonical_cache" == "$cache_root" ]] || {
    echo "upstream cache root contains symlinked components" >&2
    return 1
  }
  [[ -d "$source_dir" && ! -L "$source_dir" && -d "$source_dir/t" &&
    ! -L "$source_dir/t" ]] || {
    echo "pinned upstream source root is missing or symlinked: $source_dir" >&2
    return 1
  }
  canonical_source="$(cd -P "$source_dir" && pwd -P)" || return 1
  [[ "$canonical_source" == "$source_dir" ]] || {
    echo "pinned upstream source root contains symlinked components: $source_dir" >&2
    return 1
  }
  archive="$cache_root/$tag.tar.gz"
  marker="$source_dir/.zmin-pristine-source.sha256"
  [[ -f "$archive" && ! -L "$archive" && -s "$archive" &&
    -f "$marker" && ! -L "$marker" ]] || {
    echo "pinned upstream archive/source marker is missing or symlinked" >&2
    return 1
  }
  archive_sha="$(shasum -a 256 "$archive" | awk '{ print $1 }')" || return 1
  [[ "$archive_sha" == "$contract_archive_sha" ]] || {
    echo "pinned upstream archive SHA-256 mismatch" >&2
    return 1
  }
  marker_sha="$(tr -d '\r\n' <"$marker")" || return 1
  [[ "$marker_sha" == "$archive_sha" ]] || {
    echo "pinned upstream source marker mismatch: $source_dir" >&2
    return 1
  }
  invalid_surface="$(find "$source_dir/t" -mindepth 1 -maxdepth 1 \
    -name 't[0-9][0-9][0-9][0-9]-*.sh' ! -type f -print -quit)"
  [[ -z "$invalid_surface" ]] || {
    echo "pinned upstream direct test surface contains a non-regular entry: $invalid_surface" >&2
    return 1
  }
  find "$source_dir/t" -mindepth 1 -maxdepth 1 -type f \
    -name 't[0-9][0-9][0-9][0-9]-*.sh' -print |
    sed 's#^.*/##' | LC_ALL=C sort >"$direct_tests_tmp"
  actual_total="$(wc -l <"$direct_tests_tmp" | tr -d ' ')" || return 1
  [[ "$actual_total" == "$expected_total" ]] || {
    echo "pinned upstream direct test count mismatch: $actual_total/$expected_total" >&2
    return 1
  }
}

direct_tests_tmp="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-direct-tests.XXXXXX")"
trap 'rm -f "$direct_tests_tmp"' EXIT
validate_pinned_source || exit 2
if [[ "$mode" == "source-files" ]]; then
  cat "$direct_tests_tmp"
  rm -f "$direct_tests_tmp"
  exit 0
fi

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
deprecated_patterns_tmp="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-deprecated-patterns.XXXXXX")"
breaking_patterns_tmp="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-breaking-patterns.XXXXXX")"
marker_matches_tmp="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-marker-matches.XXXXXX")"
matches_tmp="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-deprecated-matches.XXXXXX")"
trap 'rm -f "$direct_tests_tmp" "$deprecated_tmp" "$deprecated_patterns_tmp" "$breaking_patterns_tmp" "$marker_matches_tmp" "$matches_tmp"' EXIT

cat >"$deprecated_patterns_tmp" <<'EOF'
deprecated
scheduled for removal
will be removed in Git 3\.0
--i-still-use-this
EOF

printf '%s\n' 'WITH_BREAKING_CHANGES' >"$breaking_patterns_tmp"

is_excluded_family() {
  local test_name="$1"
  awk -F '\t' -v test_name="$test_name" '
    NR == 1 || /^#/ || NF < 2 { next }
    $5 == "upstream deprecated/removed" && $2 != "" && test_name ~ ("^" $2) { found = 1 }
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
    $5 == "upstream deprecated/removed" && $2 != "" && test_name ~ ("^" $2) { print $4; exit }
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

scan_direct_tests() {
  local pattern="$1"
  awk -v root="$source_dir/t" '{ printf "%s/%s%c", root, $0, 0 }' "$direct_tests_tmp" |
    xargs -0 rg -n -i -f "$pattern" -- || true
}

{
  {
    scan_direct_tests "$deprecated_patterns_tmp"
  } |
    awk -F ':' '
      {
        file = $1
        sub(/^.*\//, "", file)
        printf "%s\tdeprecated\n", file
      }
    '
  {
    scan_direct_tests "$breaking_patterns_tmp"
  } |
    awk -F ':' '
      {
        file = $1
        sub(/^.*\//, "", file)
        printf "%s\tbreaking-change\n", file
      }
    '
} |
  LC_ALL=C sort >"$marker_matches_tmp"

awk -F '\t' '
  {
    files[$1] = 1
    if ($2 == "deprecated") {
      deprecated[$1]++
    } else if ($2 == "breaking-change") {
      breaking[$1]++
    }
  }
  END {
    for (file in files) {
      printf "%s\t%d\t%d\n", file, deprecated[file] + 0, breaking[file] + 0
    }
  }
' "$marker_matches_tmp" |
  LC_ALL=C sort >"$matches_tmp"

printf 'classification\ttest\tdeprecated_markers\tbreaking_change_markers\treason\n' >"$deprecated_tmp"

while IFS=$'\t' read -r test_name deprecated_markers breaking_change_markers; do
  [[ -z "$test_name" ]] && continue

  if (( deprecated_markers > 0 )); then
    classification="mixed-in-scope"
    reason="contains deprecated assertions inside a still-supported upstream shell test"
  else
    classification="breaking-change-in-scope"
    reason="contains WITH_BREAKING_CHANGES coverage; no deprecated/removed marker evidence"
  fi
  if is_excluded_family "$test_name"; then
    classification="fully-excluded-family"
    reason="$(family_reason "$test_name")"
  elif is_excluded_file "$test_name"; then
    classification="fully-excluded-file"
    reason="$(file_reason "$test_name")"
  fi

  printf '%s\t%s\t%s\t%s\t%s\n' \
    "$classification" \
    "$test_name" \
    "$deprecated_markers" \
    "$breaking_change_markers" \
    "$reason" >>"$deprecated_tmp"
done <"$matches_tmp"

if [[ "$mode" == "summary" ]]; then
  printf 'bucket\tcount\treason\n'
  awk -F '\t' '
    NR == 1 { next }
    {
      marker_file_names[$2] = 1
      if (($3 + 0) > 0) {
        deprecated_files++
      }
      if (($4 + 0) > 0) {
        breaking_files++
      }
      if (($3 + 0) > 0 && $1 == "fully-excluded-family") {
        deprecated_excluded_family++
      }
      if (($3 + 0) > 0 && $1 == "fully-excluded-file") {
        deprecated_excluded_file++
      }
      if (($4 + 0) > 0 && $1 == "fully-excluded-family") {
        breaking_excluded_family++
      }
      if (($4 + 0) > 0 && $1 == "fully-excluded-file") {
        breaking_excluded_file++
      }
      if (($3 + 0) > 0 && $1 == "mixed-in-scope") {
        deprecated_mixed++
      }
      if (($4 + 0) > 0 && $1 != "fully-excluded-family" && $1 != "fully-excluded-file") {
        breaking_in_scope++
      }
    }
    END {
      for (file in marker_file_names) {
        marker_files++
      }
      printf "marker_top_level_shell_files\t%d\ttop-level upstream shell tests with either deprecated/removal or WITH_BREAKING_CHANGES markers\n", marker_files
      printf "deprecated_top_level_shell_files\t%d\ttop-level upstream shell tests with explicit deprecated/removal markers; WITH_BREAKING_CHANGES is excluded\n", deprecated_files
      printf "deprecated_fully_excluded_family_files\t%d\texplicit legacy/deprecated/external family excludes that also carry deprecated markers\n", deprecated_excluded_family
      printf "deprecated_fully_excluded_file_rows\t%d\toptional per-file excludes that also carry deprecated markers\n", deprecated_excluded_file
      printf "deprecated_mixed_in_scope_files\t%d\tstill-supported upstream shell tests that keep deprecated assertions alongside live command coverage\n", deprecated_mixed
      printf "breaking_change_marker_files\t%d\ttop-level upstream shell tests containing WITH_BREAKING_CHANGES\n", breaking_files
      printf "breaking_change_fully_excluded_family_files\t%d\texcluded files that also contain WITH_BREAKING_CHANGES\n", breaking_excluded_family
      printf "breaking_change_fully_excluded_file_rows\t%d\toptional per-file excludes that also contain WITH_BREAKING_CHANGES\n", breaking_excluded_file
      printf "breaking_change_in_scope_files\t%d\tupstream shell tests containing WITH_BREAKING_CHANGES that remain in scope\n", breaking_in_scope
    }
  ' "$deprecated_tmp"
  exit 0
fi

cat "$deprecated_tmp"
