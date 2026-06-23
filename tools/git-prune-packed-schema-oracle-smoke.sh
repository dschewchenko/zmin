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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-prune-packed-schema-oracle.XXXXXX")"
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

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" repack -adq
  local head
  head="$("$GIT_BIN" -C "$repo" rev-parse HEAD)"
  "$GIT_BIN" -C "$repo" cat-file -p "$head" | "$GIT_BIN" -C "$repo" hash-object -t commit -w --stdin >/dev/null
}

loose_objects() {
  local repo="$1"
  find "$repo/.git/objects" -type f \
    ! -path '*/pack/*' \
    ! -path '*/info/*' \
    | sed "s#$repo/.git/objects/##" \
    | sort
}

run_case() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$seed"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  loose_objects "$git_work" >"$tmpdir/${name}.git.loose"
  loose_objects "$zmin_work" >"$tmpdir/${name}.zmin.loose"
  compare_files loose_objects "$tmpdir/${name}.git.loose" "$tmpdir/${name}.zmin.loose"
  "$GIT_BIN" -C "$git_work" status --short >"$tmpdir/${name}.git.status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$tmpdir/${name}.zmin.status"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case prune_packed_quiet_long prune-packed --quiet
run_case prune_packed_quiet_short prune-packed -q
run_case prune_packed_dry_run_long prune-packed --dry-run
