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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-clone-reftable-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

source_repo="$tmpdir/source"
"$GIT_BIN" init -q -b main "$source_repo"
"$GIT_BIN" -C "$source_repo" config user.name Oracle
"$GIT_BIN" -C "$source_repo" config user.email oracle@example.com
"$GIT_BIN" -C "$source_repo" config commit.gpgsign false
printf 'hello\n' >"$source_repo/README.md"
"$GIT_BIN" -C "$source_repo" add README.md
"$GIT_BIN" -C "$source_repo" commit -q -m init

git_exit=0
zmin_exit=0
set +e
"$GIT_BIN" -C "$tmpdir" clone --ref-format=reftable "$source_repo" git-reftable >"$tmpdir/git.out" 2>"$tmpdir/git.err"
git_exit=$?
"$ZMIN_BIN" -C "$tmpdir" clone --ref-format=reftable "$source_repo" zmin-reftable >"$tmpdir/zmin.out" 2>"$tmpdir/zmin.err"
zmin_exit=$?
set -e

test "$git_exit" = 0
test "$("$GIT_BIN" -C "$tmpdir/git-reftable" rev-parse --show-ref-format)" = reftable
test -d "$tmpdir/git-reftable/.git/reftable"
test ! -e "$tmpdir/git-reftable/.git/refs/heads/main"
test "$zmin_exit" = 0
test "$("$GIT_BIN" -C "$tmpdir/zmin-reftable" rev-parse --show-ref-format)" = reftable
test -d "$tmpdir/zmin-reftable/.git/reftable"
test ! -e "$tmpdir/zmin-reftable/.git/refs/heads/main"
test "$("$GIT_BIN" -C "$tmpdir/zmin-reftable" rev-parse HEAD)" = "$("$GIT_BIN" -C "$tmpdir/git-reftable" rev-parse HEAD)"
test "$("$ZMIN_BIN" -C "$tmpdir/zmin-reftable" rev-parse HEAD)" = "$("$GIT_BIN" -C "$tmpdir/git-reftable" rev-parse HEAD)"

printf 'clone_ref_format_reftable\texact\tstock_exit=%s\tzmin_exit=%s\n' "$git_exit" "$zmin_exit"
