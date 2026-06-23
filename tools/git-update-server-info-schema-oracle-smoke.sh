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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-update-server-info-oracle.XXXXXX")"
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

make_source_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m one
  "$GIT_BIN" -C "$repo" branch feature
  "$GIT_BIN" -C "$repo" tag lightweight
}

run_case() {
  local name="$1"
  shift
  local source="$tmpdir/${name}.source"
  local git_repo="$tmpdir/${name}.git.git"
  local zmin_repo="$tmpdir/${name}.zmin.git"
  local git_exit=0
  local zmin_exit=0

  make_source_repo "$source"
  "$GIT_BIN" clone -q --bare "$source" "$git_repo"
  "$GIT_BIN" clone -q --bare "$source" "$zmin_repo"
  "$GIT_BIN" -C "$git_repo" repack -adq
  "$GIT_BIN" -C "$zmin_repo" repack -adq

  set +e
  "$GIT_BIN" -C "$git_repo" update-server-info "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" update-server-info "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  compare_files info_refs "$git_repo/info/refs" "$zmin_repo/info/refs"
  compare_files packs "$git_repo/objects/info/packs" "$zmin_repo/objects/info/packs"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case update_server_info_force_long --force
