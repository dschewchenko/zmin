#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-config-oracle.XXXXXX")"
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
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  "$GIT_BIN" -C "$repo" config branch.main.remote origin
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

  set +e
  "$GIT_BIN" -C "$git_repo" config "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" config "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  compare_files config "$git_repo/.git/config" "$zmin_repo/.git/config"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case config_list_short -l
run_case config_null_short -z -l
