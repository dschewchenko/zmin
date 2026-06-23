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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-merge-index-oracle.XXXXXX")"
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

seed_unmerged_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'a\nbase\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m base

  local base
  local ours
  local theirs
  base="$("$GIT_BIN" -C "$repo" rev-parse HEAD:file.txt)"
  ours="$base"
  theirs="$(printf 'a\ntheirs\n' | "$GIT_BIN" -C "$repo" hash-object -w --stdin)"
  printf '100644 %s 1\tfile.txt\n100644 %s 2\tfile.txt\n100644 %s 3\tfile.txt\n' \
    "$base" "$ours" "$theirs" |
    "$GIT_BIN" -C "$repo" update-index --index-info
}

record_state() {
  local repo="$1"
  local prefix="$2"
  local tree_exit=0
  "$GIT_BIN" -C "$repo" status --short >"$prefix.status"
  "$GIT_BIN" -C "$repo" ls-files -s >"$prefix.index"
  set +e
  "$GIT_BIN" -C "$repo" write-tree >"$prefix.tree" 2>"$prefix.tree.err"
  tree_exit=$?
  set -e
  printf '%s\n' "$tree_exit" >"$prefix.tree.exit"
  cat "$repo/file.txt" >"$prefix.file"
}

run_exact() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  seed_unmerged_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" merge-index "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" merge-index "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  record_state "$git_work" "$tmpdir/${name}.git"
  record_state "$zmin_work" "$tmpdir/${name}.zmin"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  compare_files index "$tmpdir/${name}.git.index" "$tmpdir/${name}.zmin.index"
  compare_files tree-exit "$tmpdir/${name}.git.tree.exit" "$tmpdir/${name}.zmin.tree.exit"
  compare_files tree "$tmpdir/${name}.git.tree" "$tmpdir/${name}.zmin.tree"
  compare_files tree-stderr "$tmpdir/${name}.git.tree.err" "$tmpdir/${name}.zmin.tree.err"
  compare_files file "$tmpdir/${name}.git.file" "$tmpdir/${name}.zmin.file"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_gap() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  seed_unmerged_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" merge-index "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" merge-index "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  record_state "$git_work" "$tmpdir/${name}.git"
  record_state "$zmin_work" "$tmpdir/${name}.zmin"

  if test "$git_exit" = "$zmin_exit" &&
    cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" &&
    cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err" &&
    cmp -s "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status" &&
    cmp -s "$tmpdir/${name}.git.index" "$tmpdir/${name}.zmin.index" &&
    cmp -s "$tmpdir/${name}.git.tree.exit" "$tmpdir/${name}.zmin.tree.exit" &&
    cmp -s "$tmpdir/${name}.git.tree" "$tmpdir/${name}.zmin.tree" &&
    cmp -s "$tmpdir/${name}.git.tree.err" "$tmpdir/${name}.zmin.tree.err" &&
    cmp -s "$tmpdir/${name}.git.file" "$tmpdir/${name}.zmin.file"; then
    echo "$name unexpectedly matches stock Git; update the open matrix row" >&2
    return 1
  fi

  printf '%s\tgap\tgit_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_gap merge_index_one_shot_path -o git-merge-one-file file.txt
run_gap merge_index_quiet_path -q git-merge-one-file file.txt
run_gap merge_index_explicit_path git-merge-one-file file.txt
