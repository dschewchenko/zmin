#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d /tmp/zmin-hash-object-schema-oracle.XXXXXX)"
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
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  printf 'typed\n' >"$repo/a.txt"
}

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$git_work"
  seed_repo "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files worktree_status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_gap() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$git_work"
  seed_repo "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  if test "$git_exit" = "$zmin_exit" \
    && cmp -s "$git_out" "$zmin_out" \
    && cmp -s "$git_err" "$zmin_err"; then
    echo "$name unexpectedly matched" >&2
    exit 1
  fi
  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_case hash_object_short_type_blob hash-object -t blob a.txt
run_case hash_object_long_type_rejected hash-object --type blob a.txt
run_case hash_object_long_write_rejected hash-object --write a.txt
