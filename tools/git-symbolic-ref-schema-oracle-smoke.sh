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

tmpdir="$(mktemp -d /tmp/zmin-symbolic-ref-schema-oracle.XXXXXX)"
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
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m one
  "$GIT_BIN" -C "$repo" branch plumbing
  "$GIT_BIN" -C "$repo" symbolic-ref refs/heads/inner refs/heads/main
  "$GIT_BIN" -C "$repo" symbolic-ref refs/heads/outer refs/heads/inner
}

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_head="$tmpdir/${name}.git.head"
  local zmin_head="$tmpdir/${name}.zmin.head"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$git_work"
  seed_repo "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  cat "$git_work/.git/HEAD" >"$git_head"
  cat "$zmin_work/.git/HEAD" >"$zmin_head"
  compare_files head "$git_head" "$zmin_head"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files worktree_status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_detached_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$git_work"
  seed_repo "$zmin_work"
  git_head="$("$GIT_BIN" -C "$git_work" rev-parse HEAD)"
  zmin_head="$("$GIT_BIN" -C "$zmin_work" rev-parse HEAD)"
  "$GIT_BIN" -C "$git_work" checkout -q "$git_head"
  "$GIT_BIN" -C "$zmin_work" checkout -q "$zmin_head"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files worktree_status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case symbolic_ref_positional_name symbolic-ref HEAD
run_case symbolic_ref_positional_target symbolic-ref HEAD refs/heads/plumbing
run_case symbolic_ref_quiet_long symbolic-ref --quiet HEAD
run_case symbolic_ref_no_quiet_head symbolic-ref --no-quiet HEAD
run_case symbolic_ref_short_head symbolic-ref --short HEAD
run_case symbolic_ref_no_short_head symbolic-ref --no-short HEAD
run_case symbolic_ref_no_quiet_no_short_head symbolic-ref --no-quiet --no-short HEAD
run_case symbolic_ref_no_short_no_quiet_head symbolic-ref --no-short --no-quiet HEAD
run_case symbolic_ref_no_delete_no_quiet_head symbolic-ref --no-delete --no-quiet HEAD
run_case symbolic_ref_no_quiet_no_delete_head symbolic-ref --no-quiet --no-delete HEAD
run_case symbolic_ref_no_recurse_head symbolic-ref --no-recurse HEAD
run_case symbolic_ref_quiet_short_head symbolic-ref --quiet --short HEAD
run_case symbolic_ref_short_quiet_head symbolic-ref --short --quiet HEAD
run_case symbolic_ref_no_quiet_short_head symbolic-ref --no-quiet --short HEAD
run_case symbolic_ref_short_no_quiet_head symbolic-ref --short --no-quiet HEAD
run_case symbolic_ref_no_delete_short_head symbolic-ref --no-delete --short HEAD
run_case symbolic_ref_short_no_delete_head symbolic-ref --short --no-delete HEAD
run_case symbolic_ref_no_delete_no_recurse_head symbolic-ref --no-delete --no-recurse HEAD
run_case symbolic_ref_no_recurse_no_delete_head symbolic-ref --no-recurse --no-delete HEAD
run_case symbolic_ref_no_quiet_no_recurse_head symbolic-ref --no-quiet --no-recurse HEAD
run_case symbolic_ref_no_recurse_no_quiet_head symbolic-ref --no-recurse --no-quiet HEAD
run_case symbolic_ref_short_no_short_head symbolic-ref --short --no-short HEAD
run_case symbolic_ref_no_short_short_head symbolic-ref --no-short --short HEAD
run_case symbolic_ref_quiet_repeated_head symbolic-ref --quiet --quiet HEAD
run_case symbolic_ref_no_recurse_short_outer symbolic-ref --no-recurse --short refs/heads/outer
run_case symbolic_ref_short_no_recurse_outer symbolic-ref --short --no-recurse refs/heads/outer
run_case symbolic_ref_recurse_short_outer symbolic-ref --recurse --short refs/heads/outer
run_case symbolic_ref_recurse_no_recurse_short_outer symbolic-ref --recurse --no-recurse --short refs/heads/outer
run_case symbolic_ref_no_recurse_recurse_short_outer symbolic-ref --no-recurse --recurse --short refs/heads/outer
run_case symbolic_ref_short_quiet_no_quiet_outer symbolic-ref -q --no-quiet refs/heads/outer
run_case symbolic_ref_no_quiet_short_quiet_outer symbolic-ref --no-quiet -q refs/heads/outer
run_case symbolic_ref_no_quiet_outer symbolic-ref --no-quiet refs/heads/outer
run_case symbolic_ref_no_quiet_short_outer symbolic-ref --no-quiet --short refs/heads/outer
run_case symbolic_ref_short_no_quiet_outer symbolic-ref --short --no-quiet refs/heads/outer
run_case symbolic_ref_no_short_no_quiet_outer symbolic-ref --no-short --no-quiet refs/heads/outer
run_case symbolic_ref_no_quiet_no_short_outer symbolic-ref --no-quiet --no-short refs/heads/outer
run_case symbolic_ref_no_delete_no_quiet_outer symbolic-ref --no-delete --no-quiet refs/heads/outer
run_case symbolic_ref_no_quiet_no_delete_outer symbolic-ref --no-quiet --no-delete refs/heads/outer
run_case symbolic_ref_quiet_no_recurse_head symbolic-ref --quiet --no-recurse HEAD
run_case symbolic_ref_no_recurse_quiet_head symbolic-ref --no-recurse --quiet HEAD
run_case symbolic_ref_short_repeated_outer symbolic-ref --short --short refs/heads/outer
run_case symbolic_ref_no_recurse_repeated_outer symbolic-ref --no-recurse --no-recurse refs/heads/outer
run_case symbolic_ref_no_short_outer symbolic-ref --no-short refs/heads/outer
run_case symbolic_ref_no_short_no_recurse_outer symbolic-ref --no-short --no-recurse refs/heads/outer
run_case symbolic_ref_no_recurse_no_short_outer symbolic-ref --no-recurse --no-short refs/heads/outer
run_case symbolic_ref_short_no_short_outer symbolic-ref --short --no-short refs/heads/outer
run_case symbolic_ref_no_short_short_outer symbolic-ref --no-short --short refs/heads/outer
run_case symbolic_ref_delete_repeated_outer symbolic-ref --delete --delete refs/heads/outer
run_case symbolic_ref_no_delete_head symbolic-ref --no-delete HEAD
run_case symbolic_ref_no_delete_outer symbolic-ref --no-delete refs/heads/outer
run_case symbolic_ref_no_delete_no_short_outer symbolic-ref --no-delete --no-short refs/heads/outer
run_case symbolic_ref_no_short_no_delete_outer symbolic-ref --no-short --no-delete refs/heads/outer
run_case symbolic_ref_no_delete_short_outer symbolic-ref --no-delete --short refs/heads/outer
run_case symbolic_ref_short_no_delete_outer symbolic-ref --short --no-delete refs/heads/outer
run_case symbolic_ref_no_delete_no_recurse_outer symbolic-ref --no-delete --no-recurse refs/heads/outer
run_case symbolic_ref_no_recurse_no_delete_outer symbolic-ref --no-recurse --no-delete refs/heads/outer
run_case symbolic_ref_no_delete_inner symbolic-ref --no-delete refs/heads/inner
run_case symbolic_ref_no_delete_no_quiet_inner symbolic-ref --no-delete --no-quiet refs/heads/inner
run_case symbolic_ref_no_quiet_no_delete_inner symbolic-ref --no-quiet --no-delete refs/heads/inner
run_case symbolic_ref_no_delete_repeated_outer symbolic-ref --no-delete --no-delete refs/heads/outer
run_case symbolic_ref_delete_no_delete_outer symbolic-ref --delete --no-delete refs/heads/outer
run_case symbolic_ref_no_delete_delete_outer symbolic-ref --no-delete --delete refs/heads/outer
run_case symbolic_ref_short_delete_no_delete_outer symbolic-ref -d --no-delete refs/heads/outer
run_case symbolic_ref_no_delete_short_delete_outer symbolic-ref --no-delete -d refs/heads/outer
run_case symbolic_ref_short_delete_repeated_outer symbolic-ref -d -d refs/heads/outer
run_case symbolic_ref_delete_short_delete_outer symbolic-ref --delete -d refs/heads/outer
run_case symbolic_ref_short_delete_delete_outer symbolic-ref -d --delete refs/heads/outer
run_detached_case symbolic_ref_quiet_no_quiet_head symbolic-ref --quiet --no-quiet HEAD
run_detached_case symbolic_ref_no_quiet_quiet_head symbolic-ref --no-quiet --quiet HEAD
run_detached_case symbolic_ref_no_quiet_repeated_head symbolic-ref --no-quiet --no-quiet HEAD
run_detached_case symbolic_ref_short_quiet_no_quiet_head symbolic-ref -q --no-quiet HEAD
run_detached_case symbolic_ref_no_quiet_short_quiet_head symbolic-ref --no-quiet -q HEAD
run_detached_case symbolic_ref_short_quiet_repeated_head symbolic-ref -q -q HEAD
run_detached_case symbolic_ref_short_repeated_detached_head symbolic-ref --short --short HEAD
run_detached_case symbolic_ref_no_short_repeated_detached_head symbolic-ref --no-short --no-short HEAD
