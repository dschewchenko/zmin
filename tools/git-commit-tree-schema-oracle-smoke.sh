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

tmpdir="$(mktemp -d /tmp/zmin-commit-tree-schema-oracle.XXXXXX)"
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

seed_repo() {
  local bin="$1"
  local repo="$2"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\n' >"$repo/a.txt"
  "$bin" -C "$repo" add a.txt
}

run_case() {
  local name="$1"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  local git_tree
  local zmin_tree
  local git_exit=0
  local zmin_exit=0

  seed_repo "$GIT_BIN" "$git_work"
  seed_repo "$ZMIN_BIN" "$zmin_work"
  git_tree="$("$GIT_BIN" -C "$git_work" write-tree)"
  zmin_tree="$("$ZMIN_BIN" -C "$zmin_work" write-tree)"
  test "$git_tree" = "$zmin_tree"

  set +e
  "$GIT_BIN" -C "$git_work" commit-tree "$git_tree" -m root >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" commit-tree "$zmin_tree" -m root >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" cat-file commit "$(cat "$git_out")" >"$git_commit"
  "$GIT_BIN" -C "$zmin_work" cat-file commit "$(cat "$zmin_out")" >"$zmin_commit"
  compare_files commit_object "$git_commit" "$zmin_commit"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case commit_tree_positional_tree
