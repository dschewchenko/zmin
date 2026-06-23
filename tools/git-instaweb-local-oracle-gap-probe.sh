#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-instaweb-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

set +e
"$GIT_BIN" instaweb -h >"$tmpdir/git.out" 2>"$tmpdir/git.err"
git_exit=$?
"$ZMIN_BIN" instaweb -h >"$tmpdir/zmin.out" 2>"$tmpdir/zmin.err"
zmin_exit=$?
set -e

printf 'local_stock_git_instaweb_help\tstock_exit=%s\tzmin_exit=%s\n' "$git_exit" "$zmin_exit"
printf 'stock stderr:\n'
sed -n '1,4p' "$tmpdir/git.err"
printf 'zmin help options:\n'
sed -n '1,20p' "$tmpdir/zmin.out"

test "$git_exit" != 0
grep -q "not a git command" "$tmpdir/git.err"
test "$zmin_exit" = 0
grep -q -- "--start" "$tmpdir/zmin.out"
