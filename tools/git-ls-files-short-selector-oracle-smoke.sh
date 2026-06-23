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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-ls-files-short-oracle.XXXXXX")"
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

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'base\n' >"$repo/modified.txt"
  printf 'gone\n' >"$repo/deleted.txt"
  mkdir "$repo/dirfile"
  printf 'tracked file path\n' >"$repo/dirfile/file.txt"
  "$GIT_BIN" -C "$repo" add .
  "$GIT_BIN" -C "$repo" commit -q -m base
  printf 'changed\n' >"$repo/modified.txt"
  rm "$repo/deleted.txt"
  rm "$repo/dirfile/file.txt"
  rmdir "$repo/dirfile"
  printf 'untracked blocks tracked directory\n' >"$repo/dirfile"
}

run_case() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$seed"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" ls-files "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" ls-files "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  "$GIT_BIN" -C "$git_work" status --short >"$tmpdir/${name}.git.status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$tmpdir/${name}.zmin.status"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case ls_files_modified_short -m
run_case ls_files_deleted_short -d
run_case ls_files_killed_short -k
