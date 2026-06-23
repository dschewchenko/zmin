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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-diff-plumbing-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_rewrite_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\ntwo\nthree\nfour\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  printf 'five\nsix\nseven\neight\n' >"$repo/a.txt"
}

run_gap() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_rewrite_repo "$git_repo"
  make_rewrite_repo "$zmin_repo"

  set +e
  "$GIT_BIN" -C "$git_repo" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  if test "$git_exit" = "$zmin_exit" \
    && cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" \
    && cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"; then
    echo "$name unexpectedly matched" >&2
    exit 1
  fi
}

run_case() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_rewrite_repo "$git_repo"
  make_rewrite_repo "$zmin_repo"

  set +e
  "$GIT_BIN" -C "$git_repo" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  test "$git_exit" = "$zmin_exit"
  cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
}

run_case diff_files_break_rewrites diff-files -p --break-rewrites
run_case diff_files_reverse diff-files -p --reverse
run_case diff_index_break_rewrites diff-index -p --break-rewrites HEAD
run_case diff_index_reverse diff-index -p --reverse HEAD
