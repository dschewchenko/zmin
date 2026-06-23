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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-diff-tree-schema-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_history_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  mkdir -p "$repo/sub"
  printf 'alpha\ncommon\nneedle\nblank\n\nline\n' >"$repo/a.txt"
  printf 'beta\ncommon\nold\n' >"$repo/b.txt"
  printf 'sub old\n' >"$repo/sub/c.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m base
  printf 'alpha changed\ncommon\nneedXe\nblank\n\nline\n' >"$repo/a.txt"
  printf 'beta\ncommon\nchanged\n' >"$repo/b.txt"
  printf 'sub changed\n' >"$repo/sub/c.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m change
}

run_gap_case() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_history_repo "$git_repo"
  make_history_repo "$zmin_repo"

  set +e
  "$GIT_BIN" -C "$git_repo" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if [ "$git_exit" = "$zmin_exit" ] && cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" && cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"; then
    echo "$name unexpectedly matches stock Git; update the open matrix row" >&2
    exit 1
  fi

  test "$git_exit" = 0
  printf '%s\topen-gap\tgit_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_case() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_history_repo "$git_repo"
  make_history_repo "$zmin_repo"

  set +e
  "$GIT_BIN" -C "$git_repo" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_gap_case diff_tree_break_rewrites diff-tree -p --break-rewrites HEAD~1 HEAD
run_gap_case diff_tree_find_copies diff-tree --name-status --find-copies HEAD~1 HEAD
run_gap_case diff_tree_find_renames diff-tree --name-status --find-renames HEAD~1 HEAD
run_case diff_tree_reverse_long diff-tree --reverse -p HEAD~1 HEAD
run_gap_case diff_tree_short_B diff-tree -p -B HEAD~1 HEAD
run_gap_case diff_tree_short_C diff-tree --name-status -C HEAD~1 HEAD
run_gap_case diff_tree_short_M diff-tree --name-status -M HEAD~1 HEAD
