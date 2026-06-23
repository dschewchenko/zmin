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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-diff-files-schema-oracle.XXXXXX")"
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
  mkdir -p "$repo/sub"
  printf 'alpha\ncommon\nneedle\nblank\n\nline\n' >"$repo/a.txt"
  printf 'beta\ncommon\nold\n' >"$repo/b.txt"
  printf 'sub old\n' >"$repo/sub/c.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m base
  printf 'alpha changed\ncommon\nneedXe\nblank\n\nline\n' >"$repo/a.txt"
  printf 'beta\ncommon\nchanged\n' >"$repo/b.txt"
  printf 'sub changed\n' >"$repo/sub/c.txt"
}

run_case() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_worktree_repo "$git_repo"
  make_worktree_repo "$zmin_repo"

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

run_case diff_files_anchored diff-files -p --anchored=common
run_case diff_files_binary diff-files -p --binary
run_case diff_files_compact_summary diff-files --stat --compact-summary
run_case diff_files_default_prefix diff-files -p --default-prefix
run_case diff_files_diff_algorithm diff-files -p --diff-algorithm=histogram
run_case diff_files_find_copies_long diff-files --name-status --find-copies=50%
run_case diff_files_find_copies_harder diff-files --name-status --find-copies-harder
run_case diff_files_find_renames diff-files --name-status --find-renames
run_case diff_files_histogram diff-files -p --histogram
run_case diff_files_ignore_all_space_long diff-files -p --ignore-all-space
run_case diff_files_ignore_blank_lines diff-files -p --ignore-blank-lines
run_case diff_files_ignore_cr_at_eol diff-files -p --ignore-cr-at-eol
run_case diff_files_ignore_matching_lines_long diff-files -p --ignore-matching-lines=common
run_case diff_files_ignore_space_change diff-files -p --ignore-space-change
run_case diff_files_irreversible_delete_long diff-files -p --irreversible-delete
run_case diff_files_minimal diff-files -p --minimal
run_case diff_files_no_color_moved diff-files -p --no-color-moved
run_case diff_files_no_color_moved_ws diff-files -p --no-color-moved-ws
run_case diff_files_no_full_index diff-files --raw --no-full-index
run_case diff_files_no_relative diff-files --relative=sub --no-relative --name-only
run_case diff_files_patch_long diff-files --patch
run_case diff_files_pickaxe_all diff-files -Sneedle --pickaxe-all --name-only
run_case diff_files_pickaxe_regex diff-files -Sneed.e --pickaxe-regex --name-only
run_case diff_files_rotate_to diff-files --rotate-to=b.txt --name-only
run_case diff_files_shortstat diff-files --shortstat
run_case diff_files_summary diff-files --summary
run_case diff_files_pickaxe_G diff-files -Gchanged --name-only
run_case diff_files_short_M diff-files -M --name-status
run_case diff_files_short_a diff-files -p -a
run_case diff_files_short_b diff-files -p -b
run_case diff_files_paths diff-files --name-only -- a.txt
