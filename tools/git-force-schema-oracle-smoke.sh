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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-force-schema-oracle.XXXXXX")"
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
}

compare_common() {
  local name="$1"
  local git_work="$2"
  local zmin_work="$3"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_exit="$4"
  local zmin_exit="$5"

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --short >"$tmpdir/${name}.git.status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$tmpdir/${name}.zmin.status"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
}

run_pair() {
  local name="$1"
  local prep="$2"
  local verify="$3"
  shift 3
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$seed"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"
  "$prep" "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  compare_common "$name" "$git_work" "$zmin_work" "$git_exit" "$zmin_exit"
  "$verify" "$name" "$git_work" "$zmin_work"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

prep_rm_force() {
  printf 'two\n' >"$1/a.txt"
  printf 'two\n' >"$2/a.txt"
}

verify_index_and_worktree() {
  local name="$1"
  local git_work="$2"
  local zmin_work="$3"
  "$GIT_BIN" -C "$git_work" ls-files --stage >"$tmpdir/${name}.git.index"
  "$GIT_BIN" -C "$zmin_work" ls-files --stage >"$tmpdir/${name}.zmin.index"
  compare_files index "$tmpdir/${name}.git.index" "$tmpdir/${name}.zmin.index"
}

prep_mv_force() {
  for repo in "$1" "$2"; do
    printf 'dst\n' >"$repo/dst.txt"
    "$GIT_BIN" -C "$repo" add dst.txt
    "$GIT_BIN" -C "$repo" commit -q -m dst
  done
}

prep_tag_force() {
  for repo in "$1" "$2"; do
    "$GIT_BIN" -C "$repo" tag v1
    printf 'two\n' >"$repo/a.txt"
    "$GIT_BIN" -C "$repo" commit -q -am second
  done
}

verify_refs() {
  local name="$1"
  local git_work="$2"
  local zmin_work="$3"
  "$GIT_BIN" -C "$git_work" show-ref --tags >"$tmpdir/${name}.git.tags"
  "$GIT_BIN" -C "$zmin_work" show-ref --tags >"$tmpdir/${name}.zmin.tags"
  compare_files tags "$tmpdir/${name}.git.tags" "$tmpdir/${name}.zmin.tags"
}

prep_switch_force() {
  for repo in "$1" "$2"; do
    "$GIT_BIN" -C "$repo" switch -q -c feature
    printf 'feature\n' >"$repo/a.txt"
    "$GIT_BIN" -C "$repo" commit -q -am feature
    "$GIT_BIN" -C "$repo" switch -q main
    printf 'dirty\n' >"$repo/a.txt"
  done
}

verify_head_and_file() {
  local name="$1"
  local git_work="$2"
  local zmin_work="$3"
  "$GIT_BIN" -C "$git_work" symbolic-ref HEAD >"$tmpdir/${name}.git.head"
  "$GIT_BIN" -C "$zmin_work" symbolic-ref HEAD >"$tmpdir/${name}.zmin.head"
  compare_files head "$tmpdir/${name}.git.head" "$tmpdir/${name}.zmin.head"
  compare_files a.txt "$git_work/a.txt" "$zmin_work/a.txt"
}

run_pair rm_force_long prep_rm_force verify_index_and_worktree rm --force a.txt
run_pair rm_force_short prep_rm_force verify_index_and_worktree rm -f a.txt
run_pair mv_force_long prep_mv_force verify_index_and_worktree mv --force a.txt dst.txt
run_pair mv_force_short prep_mv_force verify_index_and_worktree mv -f a.txt dst.txt
run_pair tag_force_long prep_tag_force verify_refs tag --force v1 HEAD
run_pair tag_force_short prep_tag_force verify_refs tag -f v1 HEAD
run_pair switch_force_long prep_switch_force verify_head_and_file switch --force feature
run_pair switch_force_short prep_switch_force verify_head_and_file switch -f feature
