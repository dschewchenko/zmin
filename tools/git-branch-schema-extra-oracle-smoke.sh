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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-branch-extra-oracle.XXXXXX")"
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
  printf 'base\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" branch feature
  "$GIT_BIN" -C "$repo" update-ref refs/remotes/origin/main HEAD
}

record_state() {
  local repo="$1"
  local prefix="$2"
  "$GIT_BIN" -C "$repo" for-each-ref --format='%(refname) %(objectname)' refs/heads refs/remotes | sort >"$prefix.refs"
  "$GIT_BIN" -C "$repo" config --list --local | sort >"$prefix.config"
  "$GIT_BIN" -C "$repo" status --short >"$prefix.status"
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
  "$GIT_BIN" -C "$git_work" branch "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" branch "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  record_state "$git_work" "$tmpdir/${name}.git"
  record_state "$zmin_work" "$tmpdir/${name}.zmin"
  compare_files refs "$tmpdir/${name}.git.refs" "$tmpdir/${name}.zmin.refs"
  compare_files config "$tmpdir/${name}.git.config" "$tmpdir/${name}.zmin.config"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case branch_all_long --all
run_case branch_list_short -l
run_case branch_delete_long --delete feature
run_case branch_move_long --move feature moved
