#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-schema-parser-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

run_in_empty_dirs() {
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

  mkdir "$git_work" "$zmin_work"
  set +e
  (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  cmp -s "$git_out" "$zmin_out"
  cmp -s "$git_err" "$zmin_err"
  test ! -e "$git_work/.git"
  test ! -e "$zmin_work/.git"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_in_repo_dirs() {
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

  mkdir "$git_work" "$zmin_work"
  "$GIT_BIN" -C "$git_work" init -q
  "$GIT_BIN" -C "$zmin_work" init -q
  set +e
  (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  cmp -s "$git_out" "$zmin_out"
  cmp -s "$git_err" "$zmin_err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_in_empty_dirs init_object_format_invalid init --object-format=bogus
run_in_empty_dirs init_ref_format_invalid init --ref-format=bogus
run_in_repo_dirs config_file_missing_long config --file=/no/such/file user.name
run_in_repo_dirs config_file_missing_short config -f /no/such/file user.name
run_in_repo_dirs add_pathspec_from_file_missing_equals add --pathspec-from-file=/no/such/file
run_in_repo_dirs add_pathspec_from_file_missing_separate add --pathspec-from-file /no/such/file
