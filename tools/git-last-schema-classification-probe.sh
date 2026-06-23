#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-last-schema.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

run_p4_probe() {
  local repo="$tmpdir/p4"
  local git_exit=0
  local zmin_exit=0

  "$GIT_BIN" -C "$tmpdir" init -q "$repo"

  set +e
  (
    cd "$repo"
    "$GIT_BIN" p4 unknown
  ) >"$tmpdir/p4.git.out" 2>"$tmpdir/p4.git.err"
  git_exit=$?
  (
    cd "$repo"
    "$ZMIN_BIN" p4 unknown
  ) >"$tmpdir/p4.zmin.out" 2>"$tmpdir/p4.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 2
  test "$zmin_exit" = 129
  grep -F "unknown command unknown" "$tmpdir/p4.git.out" >/dev/null
  grep -F "fatal: unsupported p4 command 'unknown'" "$tmpdir/p4.zmin.err" >/dev/null
  printf 'p4_positional_unknown_gap\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$git_exit" "$zmin_exit"
}

run_scalar_probe() {
  local git_exit=0
  local zmin_exit=0

  set +e
  (
    cd "$tmpdir"
    "$GIT_BIN" scalar
  ) >"$tmpdir/scalar.git.out" 2>"$tmpdir/scalar.git.err"
  git_exit=$?
  (
    cd "$tmpdir"
    "$ZMIN_BIN" scalar
  ) >"$tmpdir/scalar.zmin.out" 2>"$tmpdir/scalar.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 1
  test "$zmin_exit" = 129
  grep -F "git: 'scalar' is not a git command" "$tmpdir/scalar.git.err" >/dev/null
  grep -F "usage: scalar" "$tmpdir/scalar.zmin.err" >/dev/null
  printf 'scalar_local_unavailable_gap\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$git_exit" "$zmin_exit"
}

run_p4_probe
run_scalar_probe
