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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-receive-pack-oracle.XXXXXX")"
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

run_quiet_gap() {
  local name="$1"
  shift
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
  printf '' | "$GIT_BIN" receive-pack "$@" "$git_bare" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  printf '' | "$ZMIN_BIN" receive-pack "$@" "$zmin_bare" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "128"
  test "$zmin_exit" = "0"
  grep -q "fatal: the remote end hung up unexpectedly" "$tmpdir/${name}.git.err"
  test ! -s "$tmpdir/${name}.zmin.err"
  test -s "$tmpdir/${name}.git.out"
  test -s "$tmpdir/${name}.zmin.out"
  "$GIT_BIN" --git-dir="$git_bare" show-ref >"$tmpdir/${name}.git.refs.after"
  "$GIT_BIN" --git-dir="$zmin_bare" show-ref >"$tmpdir/${name}.zmin.refs.after"
  compare_files git-refs "$tmpdir/${name}.git.refs.before" "$tmpdir/${name}.git.refs.after"
  compare_files zmin-refs "$tmpdir/${name}.zmin.refs.before" "$tmpdir/${name}.zmin.refs.after"
  printf '%s\tgap\tgit_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_quiet_gap receive_pack_quiet_long --quiet
run_quiet_gap receive_pack_quiet_short -q
