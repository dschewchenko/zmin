#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-credential-store-schema-oracle.XXXXXX")"
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

run_with_stdin() {
  local bin="$1"
  local home="$2"
  local stdin_file="$3"
  local out_file="$4"
  local err_file="$5"
  shift 5
  HOME="$home" "$bin" "$@" <"$stdin_file" >"$out_file" 2>"$err_file"
}

run_case() {
  local name="$1"
  local git_home="$tmpdir/${name}.git.home"
  local zmin_home="$tmpdir/${name}.zmin.home"
  local git_store="$tmpdir/${name}.git.credentials"
  local zmin_store="$tmpdir/${name}.zmin.credentials"
  local complete="$tmpdir/${name}.complete.stdin"
  local query="$tmpdir/${name}.query.stdin"
  local erase="$tmpdir/${name}.erase.stdin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"

  mkdir "$git_home" "$zmin_home"
  printf 'protocol=https\nhost=example.com\nusername=u\npassword=p\n\n' >"$complete"
  printf 'protocol=https\nhost=example.com\n\n' >"$query"
  printf 'protocol=https\nhost=example.com\nusername=u\n\n' >"$erase"

  run_with_stdin "$GIT_BIN" "$git_home" "$complete" "$git_out" "$git_err" \
    credential-store --file "$git_store" store
  run_with_stdin "$ZMIN_BIN" "$zmin_home" "$complete" "$zmin_out" "$zmin_err" \
    credential-store --file "$zmin_store" store
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  compare_files credentials "$git_store" "$zmin_store"
  test ! -e "$git_home/.git-credentials"
  test ! -e "$zmin_home/.git-credentials"

  run_with_stdin "$GIT_BIN" "$git_home" "$query" "$git_out" "$git_err" \
    credential-store --file "$git_store" get
  run_with_stdin "$ZMIN_BIN" "$zmin_home" "$query" "$zmin_out" "$zmin_err" \
    credential-store --file "$zmin_store" get
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"

  run_with_stdin "$GIT_BIN" "$git_home" "$erase" "$git_out" "$git_err" \
    credential-store --file "$git_store" erase
  run_with_stdin "$ZMIN_BIN" "$zmin_home" "$erase" "$zmin_out" "$zmin_err" \
    credential-store --file "$zmin_store" erase
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  compare_files credentials "$git_store" "$zmin_store"
  printf '%s\tok\texit=0\n' "$name"
}

run_case credential_store_file_actions
