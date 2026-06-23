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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-upload-pack-oracle.XXXXXX")"
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

seed_bare_repo() {
  local src="$1"
  local bare="$2"
  "$GIT_BIN" init -q -b main "$src"
  "$GIT_BIN" -C "$src" config user.name Oracle
  "$GIT_BIN" -C "$src" config user.email oracle@example.com
  printf 'one\n' >"$src/a.txt"
  "$GIT_BIN" -C "$src" add a.txt
  "$GIT_BIN" -C "$src" commit -q -m one
  "$GIT_BIN" clone -q --bare "$src" "$bare"
}

run_gap() {
  local name="$1"
  local expected_git_exit="$2"
  local expected_zmin_exit="$3"
  shift 3
  local git_src="$tmpdir/${name}.git.src"
  local zmin_src="$tmpdir/${name}.zmin.src"
  local git_bare="$tmpdir/${name}.git.git"
  local zmin_bare="$tmpdir/${name}.zmin.git"
  local git_exit=0
  local zmin_exit=0

  seed_bare_repo "$git_src" "$git_bare"
  seed_bare_repo "$zmin_src" "$zmin_bare"
  "$GIT_BIN" --git-dir="$git_bare" show-ref >"$tmpdir/${name}.git.refs.before"
  "$GIT_BIN" --git-dir="$zmin_bare" show-ref >"$tmpdir/${name}.zmin.refs.before"

  set +e
  printf '' | "$GIT_BIN" upload-pack "$@" "$git_bare" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  printf '' | "$ZMIN_BIN" upload-pack "$@" "$zmin_bare" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$expected_git_exit"
  test "$zmin_exit" = "$expected_zmin_exit"
  if test "$git_exit" = "$zmin_exit" &&
    cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" &&
    cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"; then
    echo "$name unexpectedly matches stock Git; update the open matrix row" >&2
    return 1
  fi
  "$GIT_BIN" --git-dir="$git_bare" show-ref >"$tmpdir/${name}.git.refs.after"
  "$GIT_BIN" --git-dir="$zmin_bare" show-ref >"$tmpdir/${name}.zmin.refs.after"
  compare_files git-refs "$tmpdir/${name}.git.refs.before" "$tmpdir/${name}.git.refs.after"
  compare_files zmin-refs "$tmpdir/${name}.zmin.refs.before" "$tmpdir/${name}.zmin.refs.after"
  printf '%s\tgap\tgit_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_gap upload_pack_advertise_refs 0 0 --advertise-refs
run_gap upload_pack_strict_empty_stdin 128 0 --strict
run_gap upload_pack_no_strict_empty_stdin 128 0 --no-strict
run_gap upload_pack_timeout_empty_stdin 128 0 --timeout=1
run_gap upload_pack_stateless_rpc_empty_stdin 128 0 --stateless-rpc
