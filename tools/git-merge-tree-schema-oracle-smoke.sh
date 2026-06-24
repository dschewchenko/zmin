#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-merge-tree-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

repo="$tmpdir/repo"
mkdir "$repo"
"$GIT_BIN" -C "$repo" init -q -b main
"$GIT_BIN" -C "$repo" config user.name "Oracle"
"$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
printf 'base\n' >"$repo/a.txt"
"$GIT_BIN" -C "$repo" add -A
"$GIT_BIN" -C "$repo" commit -qm "base"
base_commit="$("$GIT_BIN" -C "$repo" rev-parse HEAD)"
base_tree="$("$GIT_BIN" -C "$repo" rev-parse HEAD^{tree})"

"$GIT_BIN" -C "$repo" switch -q -c theirs
printf 'theirs\n' >"$repo/a.txt"
"$GIT_BIN" -C "$repo" add -A
"$GIT_BIN" -C "$repo" commit -qm "theirs"
theirs_tree="$("$GIT_BIN" -C "$repo" rev-parse HEAD^{tree})"

"$GIT_BIN" -C "$repo" switch -q --detach "$base_commit"
"$GIT_BIN" -C "$repo" merge-tree --trivial-merge "$base_tree" "$base_tree" "$theirs_tree" >"$tmpdir/git.out" 2>"$tmpdir/git.err"
"$ZMIN_BIN" -C "$repo" merge-tree --trivial-merge "$base_tree" "$base_tree" "$theirs_tree" >"$tmpdir/zmin.out" 2>"$tmpdir/zmin.err"
cmp -s "$tmpdir/git.out" "$tmpdir/zmin.out"
cmp -s "$tmpdir/git.err" "$tmpdir/zmin.err"
printf 'merge_tree_trivial_merge\tok\n'

run_invalid() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0
  set +e
  "$GIT_BIN" -C "$repo" merge-tree "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$repo" merge-tree "$@" >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e
  test "$git_exit" = "$zmin_exit"
  cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out"
  cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_invalid merge_tree_short_m_invalid -m "$base_tree" "$base_tree" "$theirs_tree"
run_invalid merge_tree_short_p_invalid -p "$base_tree" "$base_tree" "$theirs_tree"
run_invalid merge_tree_short_F_invalid -F "$base_tree" "$base_tree" "$theirs_tree"
