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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-clone-short-flags-oracle.XXXXXX")"
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

normalize_stream() {
  local input="$1"
  local output="$2"
  local git_dst="$3"
  local zmin_dst="$4"
  sed -e "s#${git_dst}#<dst>#g" -e "s#${zmin_dst}#<dst>#g" "$input" >"$output"
}

record_common_state() {
  local repo="$1"
  local prefix="$2"
  "$GIT_BIN" -C "$repo" rev-parse HEAD >"$prefix.head"
  "$GIT_BIN" -C "$repo" status --short >"$prefix.status"
  "$GIT_BIN" -C "$repo" config --get remote.origin.url >"$prefix.origin-url"
}

record_worktree_file_if_present() {
  local repo="$1"
  local prefix="$2"
  if test -e "$repo/a.txt"; then
    cat "$repo/a.txt" >"$prefix.a-txt"
  else
    : >"$prefix.a-txt"
  fi
}

record_normalized_alternates() {
  local repo="$1"
  local prefix="$2"
  local alternates="$repo/.git/objects/info/alternates"
  if test -f "$alternates"; then
    sed -e "s#${tmpdir}#<tmp>#g" "$alternates" >"$prefix.alternates"
  else
    : >"$prefix.alternates"
  fi
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
  normalize_stream "$tmpdir/${name}.git.out" "$tmpdir/${name}.git.out.norm" "$git_dst" "$zmin_dst"
  normalize_stream "$tmpdir/${name}.zmin.out" "$tmpdir/${name}.zmin.out.norm" "$git_dst" "$zmin_dst"
  normalize_stream "$tmpdir/${name}.git.err" "$tmpdir/${name}.git.err.norm" "$git_dst" "$zmin_dst"
  normalize_stream "$tmpdir/${name}.zmin.err" "$tmpdir/${name}.zmin.err.norm" "$git_dst" "$zmin_dst"
  compare_files stdout "$tmpdir/${name}.git.out.norm" "$tmpdir/${name}.zmin.out.norm"
  compare_files stderr "$tmpdir/${name}.git.err.norm" "$tmpdir/${name}.zmin.err.norm"

  record_common_state "$git_dst" "$tmpdir/${name}.git"
  record_common_state "$zmin_dst" "$tmpdir/${name}.zmin"
  record_worktree_file_if_present "$git_dst" "$tmpdir/${name}.git"
  record_worktree_file_if_present "$zmin_dst" "$tmpdir/${name}.zmin"
  record_normalized_alternates "$git_dst" "$tmpdir/${name}.git"
  record_normalized_alternates "$zmin_dst" "$tmpdir/${name}.zmin"
  compare_files head "$tmpdir/${name}.git.head" "$tmpdir/${name}.zmin.head"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  compare_files origin-url "$tmpdir/${name}.git.origin-url" "$tmpdir/${name}.zmin.origin-url"
  compare_files a-txt "$tmpdir/${name}.git.a-txt" "$tmpdir/${name}.zmin.a-txt"
  compare_files alternates "$tmpdir/${name}.git.alternates" "$tmpdir/${name}.zmin.alternates"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case clone_quiet_short -q
