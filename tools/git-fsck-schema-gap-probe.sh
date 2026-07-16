#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-fsck-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf 'base\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -qm "base"
  "$GIT_BIN" -C "$repo" hash-object -w --stdin >/dev/null <<<"dangling"
}

run_probe() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  cp -R "$base_seed" "$git_work"
  cp -R "$base_seed" "$zmin_work"

  set +e
  (cd "$git_work" && "$GIT_BIN" fsck "$@") >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" fsck "$@") >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  local stdout_match=0
  local stderr_match=0
  cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" && stdout_match=1
  cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err" && stderr_match=1

  printf '%s\texact\tstock_exit=%s\tzmin_exit=%s\tstdout_match=%s\tstderr_match=%s\n' \
    "$name" "$git_exit" "$zmin_exit" "$stdout_match" "$stderr_match"

  test "$git_exit" = "$zmin_exit"
  test "$stdout_match" = "1"
  test "$stderr_match" = "1"
}

base_seed="$tmpdir/base"
make_seed_repo "$base_seed"

run_probe fsck_progress --progress
run_probe fsck_verbose --verbose
run_probe fsck_verbose_short -v
