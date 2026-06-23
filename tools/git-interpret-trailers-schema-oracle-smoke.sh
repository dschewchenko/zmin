#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-interpret-trailers-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

cat >"$tmpdir/message.txt" <<'MSG'
Subject line

Body.

Acked-by: A <a@example.com>
MSG

git_exit=0
zmin_exit=0
set +e
"$GIT_BIN" \
  -c trailer.review.key=Reviewed-by \
  -c trailer.review.ifmissing=add \
  -c trailer.review.command='printf Configured' \
  interpret-trailers --only-input <"$tmpdir/message.txt" >"$tmpdir/git.out" 2>"$tmpdir/git.err"
git_exit=$?
"$ZMIN_BIN" \
  -c trailer.review.key=Reviewed-by \
  -c trailer.review.ifmissing=add \
  -c trailer.review.command='printf Configured' \
  interpret-trailers --only-input <"$tmpdir/message.txt" >"$tmpdir/zmin.out" 2>"$tmpdir/zmin.err"
zmin_exit=$?
set -e

printf 'interpret_trailers_only_input\tstock_exit=%s\tzmin_exit=%s\n' "$git_exit" "$zmin_exit"
test "$git_exit" = 0
test "$zmin_exit" = 0
cmp -s "$tmpdir/git.out" "$tmpdir/zmin.out"
cmp -s "$tmpdir/git.err" "$tmpdir/zmin.err"
