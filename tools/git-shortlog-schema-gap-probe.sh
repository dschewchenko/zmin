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

run_gap() {
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

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  printf 'stock stdout:\n'
  sed -n '1,8p' "$tmpdir/$name.git.out"
  printf 'zmin stdout:\n'
  sed -n '1,8p' "$tmpdir/$name.zmin.out"
  test "$git_exit" = 0
  test "$zmin_exit" = 0
  cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"
  if cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out"; then
    echo "$name unexpectedly matched" >&2
    return 1
  fi
}

run_gap shortlog_committer_long --committer
run_gap shortlog_committer_short -c
