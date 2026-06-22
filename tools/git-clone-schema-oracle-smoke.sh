#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-clone-schema-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

src="$tmpdir/src"
mkdir "$src"
"$GIT_BIN" -C "$src" init -q
"$GIT_BIN" -C "$src" config user.name "Oracle"
"$GIT_BIN" -C "$src" config user.email "oracle@example.com"
printf 'base\n' >"$src/file.txt"
"$GIT_BIN" -C "$src" add file.txt
"$GIT_BIN" -C "$src" commit -qm "base"

run_case() {
  local name="$1"
  shift
  local git_dst="$tmpdir/${name}.git.dst"
  local zmin_dst="$tmpdir/${name}.zmin.dst"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" clone "$@" "$src" "$git_dst" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" clone "$@" "$src" "$zmin_dst" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  cmp -s "$git_out" "$zmin_out"
  cmp -s "$git_err" "$zmin_err"
  test ! -e "$git_dst"
  test ! -e "$zmin_dst"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case clone_jobs_invalid_long --jobs=bogus
run_case clone_jobs_invalid_short -j bogus
