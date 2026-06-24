#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-mktag-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

repo="$tmpdir/repo"
mkdir "$repo"
"$GIT_BIN" -C "$repo" init -q -b main
blob="$("$GIT_BIN" -C "$repo" hash-object -w --stdin <<<"payload")"
cat >"$tmpdir/tag.txt" <<TAG
object $blob
type blob
tag v1
tagger Oracle <oracle@example.com> 1700000000 +0000

message
TAG

run_exact() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" -C "$repo" mktag "$@" <"$tmpdir/tag.txt" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$repo" mktag "$@" <"$tmpdir/tag.txt" >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out"
  cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_exact mktag_no_strict --no-strict
