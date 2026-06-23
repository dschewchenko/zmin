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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-diff-files-merge-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_worktree_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'base\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  printf 'changed\n' >"$repo/a.txt"
}

git_repo="$tmpdir/git"
zmin_repo="$tmpdir/zmin"
make_worktree_repo "$git_repo"
make_worktree_repo "$zmin_repo"

git_exit=0
zmin_exit=0
set +e
"$GIT_BIN" -C "$git_repo" diff-files -m --name-status >"$tmpdir/git.out" 2>"$tmpdir/git.err"
git_exit=$?
"$ZMIN_BIN" -C "$zmin_repo" diff-files -m --name-status >"$tmpdir/zmin.out" 2>"$tmpdir/zmin.err"
zmin_exit=$?
set -e

if [ "$git_exit" = "$zmin_exit" ] && cmp -s "$tmpdir/git.out" "$tmpdir/zmin.out" && cmp -s "$tmpdir/git.err" "$tmpdir/zmin.err"; then
  echo "diff-files -m unexpectedly matches stock Git; update the open matrix row" >&2
  exit 1
fi

test "$git_exit" = 0
grep -q '^M[[:space:]]a.txt$' "$tmpdir/git.out"
test "$zmin_exit" != 0
printf 'diff_files_merge_option\topen-gap\tgit_exit=%s\tzmin_exit=%s\n' "$git_exit" "$zmin_exit"
