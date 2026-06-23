#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

export GIT_AUTHOR_NAME=Oracle
export GIT_AUTHOR_EMAIL=oracle@example.com
export GIT_AUTHOR_DATE="1700000000 +0000"
export GIT_COMMITTER_NAME=Oracle
export GIT_COMMITTER_EMAIL=oracle@example.com
export GIT_COMMITTER_DATE="1700000000 +0000"

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-tag-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'base\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" tag -a v1 -m release
}

run_gap() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" tag "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" tag "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  printf 'stock stdout:\n'
  cat "$tmpdir/${name}.git.out"
  printf 'zmin stdout:\n'
  cat "$tmpdir/${name}.zmin.out"
  printf 'stock stderr:\n'
  cat "$tmpdir/${name}.git.err"
  printf 'zmin stderr:\n'
  cat "$tmpdir/${name}.zmin.err"

  if test "$git_exit" = "$zmin_exit" \
    && cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" \
    && cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"; then
    echo "$name unexpectedly matched" >&2
    return 1
  fi
}

run_gap tag_delete_long --delete v1
run_gap tag_verify_long --verify v1
