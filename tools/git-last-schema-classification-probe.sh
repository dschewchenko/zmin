#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
SCALAR_BIN="${SCALAR_BIN:-scalar}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-last-schema.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

compare_files() {
  local label="$1"
  local left="$2"
  local right="$3"
  if ! cmp -s "$left" "$right"; then
    echo "$label differs" >&2
    diff -u "$left" "$right" >&2 || true
    return 1
  fi
}

run_p4_probe() {
  local repo="$tmpdir/p4"
  local git_exit=0
  local zmin_exit=0
  local git_status="$tmpdir/p4.git.status"
  local zmin_status="$tmpdir/p4.zmin.status"

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

  test "$git_exit" = "$zmin_exit"
  compare_files p4_stdout "$tmpdir/p4.git.out" "$tmpdir/p4.zmin.out"
  compare_files p4_stderr "$tmpdir/p4.git.err" "$tmpdir/p4.zmin.err"
  "$GIT_BIN" -C "$repo" status --short >"$git_status"
  "$GIT_BIN" -C "$repo" status --short >"$zmin_status"
  compare_files p4_status "$git_status" "$zmin_status"
  printf 'p4_positional_unknown\texact\texit=%s\n' "$git_exit"
}

run_scalar_probe() {
  local scalar_exit=0
  local zmin_exit=0

  if ! command -v "$SCALAR_BIN" >/dev/null 2>&1; then
    printf 'scalar_root_no_subcommand\tgap\tstock_scalar=missing\n'
    return
  fi

  set +e
  (
    cd "$tmpdir"
    "$SCALAR_BIN"
  ) >"$tmpdir/scalar.git.out" 2>"$tmpdir/scalar.git.err"
  scalar_exit=$?
  (
    cd "$tmpdir"
    "$ZMIN_BIN" scalar
  ) >"$tmpdir/scalar.zmin.out" 2>"$tmpdir/scalar.zmin.err"
  zmin_exit=$?
  set -e

  test "$scalar_exit" = "$zmin_exit"
  compare_files scalar_stdout "$tmpdir/scalar.git.out" "$tmpdir/scalar.zmin.out"
  compare_files scalar_stderr "$tmpdir/scalar.git.err" "$tmpdir/scalar.zmin.err"
  printf 'scalar_root_no_subcommand\texact\texit=%s\n' "$scalar_exit"
}

run_p4_probe
run_scalar_probe
