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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-clone-progress-verbose-oracle.XXXXXX")"
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

make_source_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
}

normalize_stderr() {
  local input="$1"
  local output="$2"
  local git_dst="$3"
  local zmin_dst="$4"
  sed -e "s#${git_dst}#<dst>#g" -e "s#${zmin_dst}#<dst>#g" "$input" >"$output"
}

record_repo_state() {
  local repo="$1"
  local prefix="$2"
  "$GIT_BIN" -C "$repo" rev-parse HEAD >"$prefix.head"
  "$GIT_BIN" -C "$repo" status --short >"$prefix.status"
  "$GIT_BIN" -C "$repo" config --get remote.origin.url >"$prefix.origin-url"
  cat "$repo/a.txt" >"$prefix.a-txt"
}

run_case() {
  local name="$1"
  shift
  local src="$tmpdir/${name}.src"
  local git_dst="$tmpdir/${name}.git"
  local zmin_dst="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_source_repo "$src"

  set +e
  "$GIT_BIN" -C "$tmpdir" clone "$@" "$src" "$git_dst" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$tmpdir" clone "$@" "$src" "$zmin_dst" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  normalize_stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.git.err.norm" "$git_dst" "$zmin_dst"
  normalize_stderr "$tmpdir/${name}.zmin.err" "$tmpdir/${name}.zmin.err.norm" "$git_dst" "$zmin_dst"
  compare_files stderr "$tmpdir/${name}.git.err.norm" "$tmpdir/${name}.zmin.err.norm"

  record_repo_state "$git_dst" "$tmpdir/${name}.git"
  record_repo_state "$zmin_dst" "$tmpdir/${name}.zmin"
  compare_files head "$tmpdir/${name}.git.head" "$tmpdir/${name}.zmin.head"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  compare_files origin-url "$tmpdir/${name}.git.origin-url" "$tmpdir/${name}.zmin.origin-url"
  compare_files a-txt "$tmpdir/${name}.git.a-txt" "$tmpdir/${name}.zmin.a-txt"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case clone_no_progress_long --no-progress
run_case clone_progress_long --progress
run_case clone_verbose_long --verbose
run_case clone_verbose_short -v
