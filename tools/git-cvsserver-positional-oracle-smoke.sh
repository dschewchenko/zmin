#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-cvsserver-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

set +e
(
  cd "$tmpdir"
  "$GIT_BIN" cvsserver unknown
) >"$tmpdir/git.out" 2>"$tmpdir/git.err"
git_exit=$?
(
  cd "$tmpdir"
  "$ZMIN_BIN" cvsserver unknown
) >"$tmpdir/zmin.out" 2>"$tmpdir/zmin.err"
zmin_exit=$?
set -e

test "$git_exit" = 0
test "$zmin_exit" = 0
cmp -s "$tmpdir/git.out" "$tmpdir/zmin.out"
cmp -s "$tmpdir/git.err" "$tmpdir/zmin.err"
printf 'cvsserver_positional_unknown_noop\tok\texit=%s\n' "$git_exit"
