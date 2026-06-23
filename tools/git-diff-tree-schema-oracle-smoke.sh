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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-diff-tree-schema-oracle.XXXXXX")"
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
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case diff_tree_binary diff-tree -p --binary HEAD~1 HEAD
run_case diff_tree_default_prefix diff-tree -p --default-prefix HEAD~1 HEAD
run_case diff_tree_diff_algorithm diff-tree -p --diff-algorithm=histogram HEAD~1 HEAD
run_case diff_tree_find_copies_harder diff-tree --name-status --find-copies-harder HEAD~1 HEAD
run_case diff_tree_histogram diff-tree -p --histogram HEAD~1 HEAD
run_case diff_tree_ignore_all_space_long diff-tree -p --ignore-all-space HEAD~1 HEAD
run_case diff_tree_ignore_blank_lines diff-tree -p --ignore-blank-lines HEAD~1 HEAD
run_case diff_tree_ignore_cr_at_eol diff-tree -p --ignore-cr-at-eol HEAD~1 HEAD
run_case diff_tree_ignore_space_at_eol diff-tree -p --ignore-space-at-eol HEAD~1 HEAD
run_case diff_tree_ignore_space_change diff-tree -p --ignore-space-change HEAD~1 HEAD
run_case diff_tree_irreversible_delete diff-tree -p --irreversible-delete HEAD~1 HEAD
run_case diff_tree_minimal diff-tree -p --minimal HEAD~1 HEAD
run_case diff_tree_no_color diff-tree -p --color=always --no-color HEAD~1 HEAD
run_case diff_tree_no_color_moved diff-tree -p --no-color-moved HEAD~1 HEAD
run_case diff_tree_no_color_moved_ws diff-tree -p --no-color-moved-ws HEAD~1 HEAD
run_case diff_tree_no_ext_diff diff-tree -p --no-ext-diff HEAD~1 HEAD
run_case diff_tree_no_full_index diff-tree --raw --no-full-index HEAD~1 HEAD
run_case diff_tree_no_relative diff-tree --relative=sub --no-relative --name-only HEAD~1 HEAD
run_case diff_tree_no_textconv diff-tree -p --no-textconv HEAD~1 HEAD
run_case diff_tree_numstat diff-tree --numstat HEAD~1 HEAD
run_case diff_tree_patience diff-tree -p --patience HEAD~1 HEAD
run_case diff_tree_patch_long diff-tree --patch HEAD~1 HEAD
run_case diff_tree_pickaxe_regex diff-tree -Sneed.e --pickaxe-regex --name-only HEAD~1 HEAD
run_case diff_tree_quiet diff-tree --quiet HEAD~1 HEAD
run_case diff_tree_shortstat diff-tree --shortstat HEAD~1 HEAD
run_case diff_tree_skip_to diff-tree --skip-to=b.txt --name-only HEAD~1 HEAD
run_case diff_tree_word_diff diff-tree --word-diff=plain -p HEAD~1 HEAD
run_case diff_tree_short_D diff-tree -p -D HEAD~1 HEAD
run_case diff_tree_short_I diff-tree -p -Icommon HEAD~1 HEAD
run_case diff_tree_short_a diff-tree -p -a HEAD~1 HEAD
run_case diff_tree_short_b diff-tree -p -b HEAD~1 HEAD
run_case diff_tree_short_m diff-tree -m --name-status HEAD~1 HEAD
