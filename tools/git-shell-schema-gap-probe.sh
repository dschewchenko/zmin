#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-shell-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_bare_repo() {
  local repo="$1"
  "$GIT_BIN" init -q --bare "$repo"
}

run_probe() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" shell "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" shell "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 128
  test "$zmin_exit" = 0
  test ! -s "$tmpdir/${name}.git.out"
  grep -F "fatal:" "$tmpdir/${name}.git.err" >/dev/null
  test -s "$tmpdir/${name}.zmin.out"
  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

repo="$tmpdir/repo.git"
make_bare_repo "$repo"

run_probe shell_inline_upload_pack_gap -c "git-upload-pack $repo"
run_probe shell_split_upload_pack_gap -c git-upload-pack "$repo"
