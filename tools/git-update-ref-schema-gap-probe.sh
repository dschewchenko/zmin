#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-update-ref-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf 'base\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add file.txt
  "$GIT_BIN" -C "$repo" commit -qm "base"
  "$GIT_BIN" -C "$repo" branch old
}

base_seed="$tmpdir/base-for-oid"
make_seed_repo "$base_seed"

git_work="$tmpdir/delete-long.git"
zmin_work="$tmpdir/delete-long.zmin"
cp -R "$base_seed" "$git_work"
cp -R "$base_seed" "$zmin_work"

git_exit=0
zmin_exit=0
set +e
"$GIT_BIN" -C "$git_work" update-ref --delete refs/heads/old >"$tmpdir/delete-long.git.out" 2>"$tmpdir/delete-long.git.err"
git_exit=$?
"$ZMIN_BIN" -C "$zmin_work" update-ref --delete refs/heads/old >"$tmpdir/delete-long.zmin.out" 2>"$tmpdir/delete-long.zmin.err"
zmin_exit=$?
set -e

"$GIT_BIN" -C "$git_work" for-each-ref --format='%(refname)%00%(objectname)' >"$tmpdir/delete-long.git.refs"
"$GIT_BIN" -C "$zmin_work" for-each-ref --format='%(refname)%00%(objectname)' >"$tmpdir/delete-long.zmin.refs"

printf 'update_ref_delete_long\tstock_exit=%s\tzmin_exit=%s\n' "$git_exit" "$zmin_exit"
printf 'stock stderr:\n'
sed -n '1,8p' "$tmpdir/delete-long.git.err"
printf 'zmin stderr:\n'
sed -n '1,8p' "$tmpdir/delete-long.zmin.err"
test "$git_exit" = 129
test "$zmin_exit" = 129
if ! cmp -s "$tmpdir/delete-long.git.out" "$tmpdir/delete-long.zmin.out" \
  || ! cmp -s "$tmpdir/delete-long.git.err" "$tmpdir/delete-long.zmin.err" \
  || ! cmp -s "$tmpdir/delete-long.git.refs" "$tmpdir/delete-long.zmin.refs"; then
  echo "update_ref_delete_long mismatch" >&2
  exit 1
fi
