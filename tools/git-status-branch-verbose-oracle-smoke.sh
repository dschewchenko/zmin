#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-status-branch-verbose.XXXXXX")"
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

write_stable_index_debug() {
  local repo="$1"
  local out="$2"
  "$GIT_BIN" -C "$repo" ls-files --stage --debug \
    | sed -E '/^[[:space:]]+(ctime|mtime|dev|ino|uid|gid|size):/d' >"$out"
}

make_dirty_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'base\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -qm base
  printf 'dirty\n' >"$repo/a.txt"
  mkdir "$repo/untracked"
  printf 'u\n' >"$repo/untracked/u.txt"
}

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_index="$tmpdir/${name}.git.index"
  local zmin_index="$tmpdir/${name}.zmin.index"
  local git_exit=0
  local zmin_exit=0

  make_dirty_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  (cd "$git_work" && "$GIT_BIN" status "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" status "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files status "$git_status" "$zmin_status"
  write_stable_index_debug "$git_work" "$git_index"
  write_stable_index_debug "$zmin_work" "$zmin_index"
  compare_files index "$git_index" "$zmin_index"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case status_branch_short_long --branch --short
run_case status_branch_short_short -b --short
run_case status_short_branch_short --short -b
run_case status_long_branch_long --long --branch
run_case status_branch_long_long --branch --long
run_case status_verbose_branch_long --verbose --branch
run_case status_branch_verbose_long --branch --verbose
run_case status_verbose_short_branch -v -b
run_case status_verbose_no_branch_long --verbose --no-branch
run_case status_no_branch_verbose_long --no-branch --verbose
run_case status_verbose_short_no_branch -v --no-branch
run_case status_long_no_branch_long --long --no-branch
run_case status_no_branch_long_long --no-branch --long
run_case status_branch_no_branch --branch --no-branch
run_case status_no_branch_branch --no-branch --branch
run_case status_branch_short_no_branch -b --no-branch
run_case status_no_branch_branch_short --no-branch -b
run_case status_short_no_branch --short --no-branch
run_case status_no_branch_short --no-branch --short
run_case status_short_branch_no_branch --short --branch --no-branch
