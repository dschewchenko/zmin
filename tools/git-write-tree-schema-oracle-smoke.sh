#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d /tmp/zmin-write-tree-schema-oracle.XXXXXX)"
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

seed_repo_with_missing_index_blob() {
  local bin="$1"
  local repo="$2"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  printf 'one\n' >"$repo/a.txt"
  "$bin" -C "$repo" add a.txt
  local blob
  blob="$("$GIT_BIN" -C "$repo" hash-object a.txt)"
  rm -f "$repo/.git/objects/${blob:0:2}/${blob:2}"
}

run_case() {
  local name="$1"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_tree_type="$tmpdir/${name}.git.tree-type"
  local zmin_tree_type="$tmpdir/${name}.zmin.tree-type"
  local git_exit=0
  local zmin_exit=0

  seed_repo_with_missing_index_blob "$GIT_BIN" "$git_work"
  seed_repo_with_missing_index_blob "$ZMIN_BIN" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" write-tree --missing-ok >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" write-tree --missing-ok >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" cat-file -t "$(cat "$git_out")" >"$git_tree_type"
  "$GIT_BIN" -C "$zmin_work" cat-file -t "$(cat "$zmin_out")" >"$zmin_tree_type"
  compare_files tree_type "$git_tree_type" "$zmin_tree_type"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case write_tree_missing_ok
