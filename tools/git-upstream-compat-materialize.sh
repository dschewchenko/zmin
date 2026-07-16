#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-upstream-compat-materialize.sh [all-top-level|all-nondeprecated|full-core|fully-excluded-deprecated] <destination>

Materializes a runnable upstream Git t/ subtree into a scratch destination
without vendoring the upstream test suite into this repository.

Modes:
  all-top-level                Copy the shared upstream t/ harness plus every
                              top-level shell test from the pinned upstream
                              source.
  all-nondeprecated           Copy the shared upstream t/ harness plus every
                              top-level shell test except explicit whole-file
                              deprecated excludes.
  full-core                   Copy the shared upstream t/ harness plus every
                              top-level shell test in the generated full-core
                              manifest.
  fully-excluded-deprecated   Copy the shared upstream t/ harness plus only the
                              fully excluded deprecated top-level shell tests.

Environment:
  ZMIN_UPSTREAM_GIT_TAG       Upstream Git tag. Default: v2.55.0.
  ZMIN_UPSTREAM_GIT_CACHE     Cache dir for upstream Git source/build.
EOF
}

mode="${1:-}"
dest="${2:-}"
case "$mode" in
  all-top-level|all-nondeprecated|full-core|fully-excluded-deprecated) ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

if [[ -z "$dest" ]]; then
  usage
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="${ZMIN_UPSTREAM_GIT_TAG:-v2.55.0}"
cache_root="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
source_dir="$cache_root/git-$tag"
manifest_tool="$repo_root/tools/git-upstream-compat-manifest.sh"
deprecated_tool="$repo_root/tools/git-upstream-deprecated-audit.sh"

if [[ ! -d "$source_dir/t" ]]; then
  echo "missing upstream Git source tree: $source_dir" >&2
  echo "run tools/git-upstream-compat-suite.sh once or populate ZMIN_UPSTREAM_GIT_CACHE first" >&2
  exit 2
fi

dest="$(cd "$(dirname "$dest")" && pwd)/$(basename "$dest")"
rm -rf "$dest"
mkdir -p "$dest/t"

manifest_tmp="$(mktemp "${TMPDIR:-/tmp}/zmin-upstream-materialize-manifest.XXXXXX")"
trap 'rm -f "$manifest_tmp"' EXIT

case "$mode" in
  all-top-level)
    "$manifest_tool" all-top-level >"$manifest_tmp"
    ;;
  all-nondeprecated)
    "$manifest_tool" all-nondeprecated >"$manifest_tmp"
    ;;
  full-core)
    "$manifest_tool" full-core >"$manifest_tmp"
    ;;
  fully-excluded-deprecated)
    {
      printf 'mode\ttest\treason\n'
      "$deprecated_tool" audit |
        awk -F '\t' '
          NR == 1 { next }
          $1 == "fully-excluded-family" || $1 == "fully-excluded-file" {
            printf "excluded\t%s\t%s\n", $2, $4
          }
        '
    } >"$manifest_tmp"
    ;;
esac

# Copy the shared harness once, but leave top-level shell tests under manifest control.
(
  cd "$source_dir/t"
  find . -mindepth 1 \
    ! -maxdepth 1 -o \
    ! -name 't[0-9][0-9][0-9][0-9]-*.sh'
) | while IFS= read -r path; do
  src="$source_dir/t/${path#./}"
  dst="$dest/t/${path#./}"
  if [[ -d "$src" ]]; then
    mkdir -p "$dst"
  else
    mkdir -p "$(dirname "$dst")"
    cp "$src" "$dst"
  fi
done

awk -F '\t' 'NR > 1 { print $2 }' "$manifest_tmp" |
  while IFS= read -r test_name; do
    [[ -z "$test_name" ]] && continue
    cp "$source_dir/t/$test_name" "$dest/t/$test_name"
  done

cp "$manifest_tmp" "$dest/manifest.tsv"

printf 'materialized\t%s\t%s\n' "$mode" "$dest"
printf 'top_level_tests\t%s\n' "$(awk -F '\t' 'NR > 1 { count += 1 } END { print count + 0 }' "$manifest_tmp")"
