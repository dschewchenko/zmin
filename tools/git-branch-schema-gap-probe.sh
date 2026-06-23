#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-branch-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

run_gap() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" branch "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  "$ZMIN_BIN" branch "$@" >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  printf 'stock stdout:\n'
  sed -n '1,8p' "$tmpdir/$name.git.out"
  printf 'stock stderr:\n'
  sed -n '1,8p' "$tmpdir/$name.git.err"
  printf 'zmin stdout:\n'
  sed -n '1,8p' "$tmpdir/$name.zmin.out"
  printf 'zmin stderr:\n'
  sed -n '1,8p' "$tmpdir/$name.zmin.err"

  if test "$git_exit" = "$zmin_exit" \
    && cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out" \
    && cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"; then
    echo "$name unexpectedly matched" >&2
    return 1
  fi
}

run_exact() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" branch "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  "$ZMIN_BIN" branch "$@" >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  test "$git_exit" = "$zmin_exit"
  if ! cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out" \
    || ! cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"; then
    echo "$name mismatch" >&2
    diff -u "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out" >&2 || true
    diff -u "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err" >&2 || true
    return 1
  fi
}

run_exact branch_help_long --help
run_exact branch_help_short -h
