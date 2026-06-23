#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-restore-oracle.XXXXXX")"
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

init_repo() {
  local repo="$1"
  "$GIT_BIN" init -q "$repo"
  "$GIT_BIN" -C "$repo" config user.email a@example.com
  "$GIT_BIN" -C "$repo" config user.name A
  printf 'old\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m old
  printf 'new\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m new
}

snapshot_repo() {
  local repo="$1"
  local out_prefix="$2"
  "$GIT_BIN" -C "$repo" status --porcelain=v1 >"${out_prefix}.status"
  "$GIT_BIN" -C "$repo" ls-files --stage >"${out_prefix}.ls_files"
  cat "$repo/a.txt" >"${out_prefix}.a_content"
}

run_case() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  init_repo "$git_repo"
  init_repo "$zmin_repo"
  printf 'dirty\n' >"$git_repo/a.txt"
  printf 'dirty\n' >"$zmin_repo/a.txt"

  set +e
  "$GIT_BIN" -C "$git_repo" restore "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" restore "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  snapshot_repo "$git_repo" "$tmpdir/${name}.git"
  snapshot_repo "$zmin_repo" "$tmpdir/${name}.zmin"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  compare_files ls-files "$tmpdir/${name}.git.ls_files" "$tmpdir/${name}.zmin.ls_files"
  compare_files worktree-content "$tmpdir/${name}.git.a_content" "$tmpdir/${name}.zmin.a_content"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case restore_worktree_short -W a.txt
run_case restore_source_short -s HEAD~1 -W a.txt
