#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-tail-positional-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

run_probe() {
  local name="$1"
  shift
  local command="$1"
  shift
  local git_exit=0
  local zmin_exit=0

  set +e
  (
    cd "$tmpdir"
    "$GIT_BIN" "$command" "$@"
  ) >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (
    cd "$tmpdir"
    "$ZMIN_BIN" "$command" "$@"
  ) >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  case "$name" in
    submodule_status_outside_repo_gap)
      test "$git_exit" = 128
      test "$zmin_exit" = 128
      cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
      cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
      printf '%s\tok\texit=%s\n' "$name" "$git_exit"
      return
      ;;
    replay_head_outside_repo_gap)
      test "$git_exit" = 128
      test "$zmin_exit" = 129
      grep -F "fatal: not a git repository (or any of the parent directories): .git" "$tmpdir/${name}.git.err" >/dev/null
      grep -F "error: exactly one of --onto, --advance, or --revert is required" "$tmpdir/${name}.zmin.err" >/dev/null
      ;;
    *)
      echo "unknown probe case: $name" >&2
      return 1
      ;;
  esac

  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_probe submodule_status_outside_repo_gap submodule status
run_probe replay_head_outside_repo_gap replay HEAD
