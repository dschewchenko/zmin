#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-verify-pack-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

repo="$tmpdir/repo"
"$GIT_BIN" init -q -b main "$repo"
"$GIT_BIN" -C "$repo" config user.name "Oracle"
"$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
printf 'content\n' >"$repo/a.txt"
"$GIT_BIN" -C "$repo" add a.txt
"$GIT_BIN" -C "$repo" commit -qm "initial"
"$GIT_BIN" -C "$repo" repack -adq
idx="$(find "$repo/.git/objects/pack" -name '*.idx' | head -1)"

run_case() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" -C "$repo" verify-pack "$@" "$idx" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$repo" verify-pack "$@" "$idx" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  test "$git_exit" = 0
  test "$zmin_exit" = 0
  cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
}

run_case verify_pack_object_format_sha1 --object-format=sha1
run_case verify_pack_verbose_long --verbose
run_case verify_pack_stat_only_long --stat-only
