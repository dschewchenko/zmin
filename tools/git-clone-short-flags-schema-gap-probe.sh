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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-clone-short-flags-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

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

run_gap() {
  local name="$1"
  local flag="$2"
  local src="$tmpdir/${name}.src"
  local git_dst="$tmpdir/${name}.git"
  local zmin_dst="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_source_repo "$src"

  set +e
  "$GIT_BIN" -C "$tmpdir" clone "$flag" "$src" "$git_dst" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$tmpdir" clone "$flag" "$src" "$zmin_dst" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 0
  test "$zmin_exit" = 0
  normalize_stream "$tmpdir/${name}.git.out" "$tmpdir/${name}.git.out.norm" "$git_dst" "$zmin_dst"
  normalize_stream "$tmpdir/${name}.zmin.out" "$tmpdir/${name}.zmin.out.norm" "$git_dst" "$zmin_dst"
  cmp -s "$tmpdir/${name}.git.out.norm" "$tmpdir/${name}.zmin.out.norm"
  normalize_stream "$tmpdir/${name}.git.err" "$tmpdir/${name}.git.err.norm" "$git_dst" "$zmin_dst"
  normalize_stream "$tmpdir/${name}.zmin.err" "$tmpdir/${name}.zmin.err.norm" "$git_dst" "$zmin_dst"
  grep -qx "Cloning into '<dst>'..." "$tmpdir/${name}.zmin.err.norm"
  grep -qx "done." "$tmpdir/${name}.git.err.norm"
  "$GIT_BIN" -C "$git_dst" rev-parse HEAD >"$tmpdir/${name}.git.head"
  "$GIT_BIN" -C "$zmin_dst" rev-parse HEAD >"$tmpdir/${name}.zmin.head"
  cmp -s "$tmpdir/${name}.git.head" "$tmpdir/${name}.zmin.head"
  "$GIT_BIN" -C "$git_dst" status --short >"$tmpdir/${name}.git.status"
  "$GIT_BIN" -C "$zmin_dst" status --short >"$tmpdir/${name}.zmin.status"
  cmp -s "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  printf '%s\tgap\tstock_stderr_has_done=zmin_missing_done\n' "$name"
}

run_gap clone_local_short -l
run_gap clone_no_checkout_short -n
run_gap clone_shared_short -s
