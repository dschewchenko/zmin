#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-add-oracle.XXXXXX")"
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

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf 'base\n' >"$repo/tracked.txt"
  "$GIT_BIN" -C "$repo" add tracked.txt
  "$GIT_BIN" -C "$repo" commit -qm "base"
}

prepare_case() {
  local work="$1"
  local name="$2"
  cp -R "$base_seed" "$work"
  case "$name" in
    add_intent_long|add_intent_short)
      printf 'intent\n' >"$work/intent.txt"
      ;;
    add_positional_path)
      printf 'new\n' >"$work/new.txt"
      ;;
    add_pathspec_file_nul)
      mkdir -p "$work/dir"
      printf 'one\n' >"$work/dir/one.txt"
      printf 'two\n' >"$work/dir/two.txt"
      printf 'dir/one.txt\0dir/two.txt\0' >"$work/paths.nul"
      ;;
  esac
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

  prepare_case "$git_work" "$name"
  prepare_case "$zmin_work" "$name"

  set +e
  (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
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

base_seed="$tmpdir/base"
make_seed_repo "$base_seed"

run_case add_intent_long add --intent-to-add intent.txt
run_case add_intent_short add -N intent.txt
run_case add_positional_path add new.txt
