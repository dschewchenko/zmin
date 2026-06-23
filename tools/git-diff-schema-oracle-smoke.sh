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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-diff-schema-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

compare_files() {
  local label="$1"
  local left="$2"
  local right="$3"
  if ! cmp -s "$left" "$right"; then
    echo "$label differs" >&2
    diff -u "$left" "$right" >&2 || true
    return 1
  fi
}

make_worktree_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\n\nkeep\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  printf 'one\n\nkeep changed\n' >"$repo/a.txt"
}

make_rename_repo() {
  local repo="$1"
  make_worktree_repo "$repo"
  "$GIT_BIN" -C "$repo" checkout -q -- .
  "$GIT_BIN" -C "$repo" mv a.txt b.txt
}

make_merge_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'base\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" checkout -q -b side
  printf 'side\n' >"$repo/side.txt"
  "$GIT_BIN" -C "$repo" add side.txt
  "$GIT_BIN" -C "$repo" commit -q -m side
  "$GIT_BIN" -C "$repo" checkout -q main
  printf 'main\n' >"$repo/main.txt"
  "$GIT_BIN" -C "$repo" add main.txt
  "$GIT_BIN" -C "$repo" commit -q -m main
  "$GIT_BIN" -C "$repo" merge -q --no-ff side -m merge
}

run_case() {
  local name="$1"
  local repo_builder="$2"
  shift 2
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  "$repo_builder" "$git_repo"
  "$repo_builder" "$zmin_repo"

  set +e
  "$GIT_BIN" -C "$git_repo" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_gap() {
  local name="$1"
  local repo_builder="$2"
  shift 2
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  "$repo_builder" "$git_repo"
  "$repo_builder" "$zmin_repo"

  set +e
  "$GIT_BIN" -C "$git_repo" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if test "$git_exit" = "$zmin_exit" &&
    cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" &&
    cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"; then
    echo "$name unexpectedly matches stock Git; update the open matrix row" >&2
    return 1
  fi

  printf '%s\tgap\tgit_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_case diff_compact_summary make_worktree_repo diff --stat --compact-summary
run_case diff_default_prefix make_worktree_repo diff --default-prefix
run_case diff_ignore_blank_lines make_worktree_repo diff --ignore-blank-lines
run_case diff_no_color_moved_ws make_worktree_repo diff --no-color-moved-ws
run_case diff_no_full_index make_worktree_repo diff --raw --no-full-index
run_case diff_no_renames make_rename_repo diff --cached --name-status --no-renames
run_gap diff_reverse_long make_worktree_repo diff --reverse -p
run_case diff_dense_combined make_merge_repo diff --cc HEAD
