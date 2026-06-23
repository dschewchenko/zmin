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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-diff-index-schema-oracle.XXXXXX")"
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

run_case diff_index_anchored diff-index -p --anchored=common HEAD
run_case diff_index_binary diff-index -p --binary HEAD
run_case diff_index_color diff-index -p --color=never HEAD
run_case diff_index_compact_summary diff-index --stat --compact-summary HEAD
run_case diff_index_default_prefix diff-index -p --default-prefix HEAD
run_case diff_index_find_copies_harder diff-index --name-status --find-copies-harder HEAD
run_case diff_index_histogram diff-index -p --histogram HEAD
run_case diff_index_ignore_all_space diff-index -p --ignore-all-space HEAD
run_case diff_index_ignore_blank_lines diff-index -p --ignore-blank-lines HEAD
run_case diff_index_ignore_cr_at_eol diff-index -p --ignore-cr-at-eol HEAD
run_case diff_index_ignore_space_at_eol diff-index -p --ignore-space-at-eol HEAD
run_case diff_index_minimal diff-index -p --minimal HEAD
run_case diff_index_no_color diff-index -p --color=always --no-color HEAD
run_case diff_index_no_color_moved_ws diff-index -p --no-color-moved-ws HEAD
run_case diff_index_no_ext_diff diff-index -p --no-ext-diff HEAD
run_case diff_index_no_full_index diff-index --raw --no-full-index HEAD
run_case diff_index_no_prefix diff-index -p --no-prefix HEAD
run_case diff_index_no_relative diff-index --relative=sub --no-relative --name-only HEAD
run_case diff_index_no_textconv diff-index -p --no-textconv HEAD
run_case diff_index_patience diff-index -p --patience HEAD
run_case diff_index_pickaxe_all diff-index -Sneedle --pickaxe-all --name-only HEAD
run_case diff_index_pickaxe_regex diff-index -Sneed.e --pickaxe-regex --name-only HEAD
run_case diff_index_quiet diff-index --quiet HEAD
run_case diff_index_root diff-index --root HEAD
run_case diff_index_shortstat diff-index --shortstat HEAD
run_case diff_index_skip_to diff-index --skip-to=b.txt --name-only HEAD
run_case diff_index_summary diff-index --summary HEAD
run_case diff_index_word_diff diff-index --word-diff=plain -p HEAD
run_case diff_index_pickaxe_G diff-index -Gchanged --name-only HEAD
run_case diff_index_short_w diff-index -p -w HEAD
run_case diff_index_paths diff-index --name-only HEAD -- a.txt
