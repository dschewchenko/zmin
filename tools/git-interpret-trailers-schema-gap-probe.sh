#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-interpret-trailers-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

cat >"$tmpdir/patch-message.txt" <<'MSG'
Subject line

Body.
---
 file.txt | 1 +
 1 file changed, 1 insertion(+)

Acked-by: Patch <patch@example.com>
MSG

git_exit=0
zmin_exit=0
set +e
"$GIT_BIN" interpret-trailers --divider <"$tmpdir/patch-message.txt" >"$tmpdir/git.out" 2>"$tmpdir/git.err"
git_exit=$?
"$ZMIN_BIN" interpret-trailers --divider <"$tmpdir/patch-message.txt" >"$tmpdir/zmin.out" 2>"$tmpdir/zmin.err"
zmin_exit=$?
set -e

printf 'interpret_trailers_divider\tstock_exit=%s\tzmin_exit=%s\n' "$git_exit" "$zmin_exit"
test "$git_exit" = "$zmin_exit"
if ! cmp -s "$tmpdir/git.out" "$tmpdir/zmin.out" \
  || ! cmp -s "$tmpdir/git.err" "$tmpdir/zmin.err"; then
  echo "interpret_trailers_divider mismatch" >&2
  diff -u "$tmpdir/git.out" "$tmpdir/zmin.out" >&2 || true
  diff -u "$tmpdir/git.err" "$tmpdir/zmin.err" >&2 || true
  exit 1
fi
