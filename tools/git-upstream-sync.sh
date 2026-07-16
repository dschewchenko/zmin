#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-upstream-sync.sh [nondeprecated|full-core|all-top-level|deprecated-only|refresh-all] [destination-root]

Creates a local, gitignored upstream Git shell-suite snapshot under the current
repository so the selected upstream tests are available without re-materializing
into ad-hoc tmp directories.

Modes:
  nondeprecated   sync the top-level upstream shell suite minus explicit
                  whole-file deprecated excludes
  full-core       sync the top-level upstream shell suite minus explicit
                  legacy/external excludes
  all-top-level   sync the complete top-level upstream shell suite
  deprecated-only sync only the fully excluded deprecated top-level shell tests
  refresh-all     sync all of the above into sibling directories

Destination root:
  default: .upstream-snapshots/git-<tag>

Environment:
  ZMIN_UPSTREAM_GIT_TAG    Upstream Git tag. Default: v2.55.0.
  ZMIN_UPSTREAM_GIT_CACHE  Cache dir for upstream Git source/build.
EOF
}

mode="${1:-nondeprecated}"
case "$mode" in
  nondeprecated|full-core|all-top-level|deprecated-only|refresh-all) ;;
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
dest_root="${2:-$repo_root/.upstream-snapshots/git-$tag}"
materialize_tool="$repo_root/tools/git-upstream-compat-materialize.sh"

if [[ ! -x "$materialize_tool" ]]; then
  echo "missing materialize tool: $materialize_tool" >&2
  exit 2
fi

dest_root="$(mkdir -p "$dest_root" && cd "$dest_root" && pwd)"

sync_one() {
  local materialize_mode="$1"
  local name="$2"
  local dest="$dest_root/$name"

  "$materialize_tool" "$materialize_mode" "$dest" >/dev/null
  printf 'synced\t%s\t%s\n' "$name" "$dest"
  awk -F '\t' '
    NR == 1 { next }
    { count += 1 }
    END { printf "top_level_tests\t%s\t%d\n", "'"$name"'", count + 0 }
  ' "$dest/manifest.tsv"
}

case "$mode" in
  nondeprecated)
    sync_one all-nondeprecated nondeprecated
    ;;
  full-core)
    sync_one full-core full-core
    ;;
  all-top-level)
    sync_one all-top-level all-top-level
    ;;
  deprecated-only)
    sync_one fully-excluded-deprecated deprecated-only
    ;;
  refresh-all)
    sync_one all-nondeprecated nondeprecated
    sync_one full-core full-core
    sync_one all-top-level all-top-level
    sync_one fully-excluded-deprecated deprecated-only
    ;;
esac
