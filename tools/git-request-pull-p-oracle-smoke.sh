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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-request-pull-p.XXXXXX")"
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
  local name="$1"
  local remote="$tmpdir/${name}.remote.git"
  local repo="$tmpdir/${name}.repo"
  "$GIT_BIN" init -q --bare "$remote"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  "$GIT_BIN" -C "$repo" remote add origin "$remote"
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -qm one
  "$GIT_BIN" -C "$repo" push -q -u origin main
  local start
  start="$("$GIT_BIN" -C "$repo" rev-parse HEAD)"
  printf 'two\n' >"$repo/b.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -qm two
  "$GIT_BIN" -C "$repo" push -q origin main
  printf '%s\t%s\tfile://%s\n' "$repo" "$start" "$remote"
}

run_case() {
  local name="$1"
  shift
  local fixture
  fixture="$(seed_repo "$name")"
  local repo start url
  IFS=$'\t' read -r repo start url <<<"$fixture"
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" -C "$repo" request-pull "$@" "$start" "$url" main >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$repo" request-pull "$@" "$start" "$url" main >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if [ "$git_exit" != "$zmin_exit" ]; then
    echo "$name exit differs: stock=$git_exit zmin=$zmin_exit" >&2
    return 1
  fi
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case request_pull_patch_short -p
run_case request_pull_patch_repeated -p -p
run_case request_pull_patch_rejects_value -p=true
