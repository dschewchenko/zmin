#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-shortlog-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

repo="$tmpdir/repo"
"$GIT_BIN" init -q -b main "$repo"
"$GIT_BIN" -C "$repo" config user.name "Committer"
"$GIT_BIN" -C "$repo" config user.email "committer@example.com"
printf 'one\n' >"$repo/a.txt"
"$GIT_BIN" -C "$repo" add a.txt
GIT_AUTHOR_NAME=Alice GIT_AUTHOR_EMAIL=alice@example.com "$GIT_BIN" -C "$repo" commit -qm "one"
printf 'two\n' >"$repo/a.txt"
GIT_AUTHOR_NAME=Bob GIT_AUTHOR_EMAIL=bob@example.com "$GIT_BIN" -C "$repo" commit -am "two" -q

run_case() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0
  set +e
  "$GIT_BIN" -C "$repo" shortlog "$@" HEAD >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$repo" shortlog "$@" HEAD >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 0
  test "$zmin_exit" = 0
  cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out"
  cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case shortlog_committer_long --committer
run_case shortlog_committer_short -c
