#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-stage-gap.XXXXXX")"
cleanup() {
  chmod -R u+rwX "$tmpdir" 2>/dev/null || true
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf '*.ignored\n' >"$repo/.gitignore"
  "$GIT_BIN" -C "$repo" add .gitignore
  "$GIT_BIN" -C "$repo" commit -qm "base"
  printf 'readable\n' >"$repo/readable.txt"
  mkdir "$repo/unreadable"
  printf 'blocked\n' >"$repo/unreadable/file.txt"
  chmod 000 "$repo/unreadable"
}

run_case() {
  local name="$1"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_repo "$git_work"
  make_repo "$zmin_work"

  set +e
  (cd "$git_work" && "$GIT_BIN" stage --ignore-errors .) >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" stage --ignore-errors .) >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  "$GIT_BIN" -C "$git_work" status --short >"$tmpdir/${name}.git.status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$tmpdir/${name}.zmin.status"

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  test "$git_exit" = "$zmin_exit"
  cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  cmp -s "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
}

run_case stage_ignore_errors_unreadable_sibling
