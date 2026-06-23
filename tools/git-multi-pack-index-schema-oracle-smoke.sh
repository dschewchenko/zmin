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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-multi-pack-index-oracle.XXXXXX")"
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

seed_two_pack_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com

  printf 'one\n' >"$repo/one.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m one
  "$GIT_BIN" -C "$repo" rev-list --objects --no-object-names HEAD |
    "$GIT_BIN" -C "$repo" pack-objects .git/objects/pack/pack >/dev/null

  export GIT_AUTHOR_DATE="1700000001 +0000"
  export GIT_COMMITTER_DATE="1700000001 +0000"
  printf 'two\n' >"$repo/two.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m two
  "$GIT_BIN" -C "$repo" rev-list --objects --no-object-names --all |
    "$GIT_BIN" -C "$repo" pack-objects .git/objects/pack/pack >/dev/null
  export GIT_AUTHOR_DATE="1700000000 +0000"
  export GIT_COMMITTER_DATE="1700000000 +0000"
}

run_object_dir_exact() {
  local name="$1"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0

  seed_two_pack_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" multi-pack-index --object-dir=.git/objects write >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" multi-pack-index --object-dir=.git/objects write >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  test -f "$git_work/.git/objects/pack/multi-pack-index"
  test -f "$zmin_work/.git/objects/pack/multi-pack-index"
  "$GIT_BIN" -C "$git_work" multi-pack-index --object-dir=.git/objects verify >"$tmpdir/${name}.git.verify.out" 2>"$tmpdir/${name}.git.verify.err"
  "$GIT_BIN" -C "$zmin_work" multi-pack-index --object-dir=.git/objects verify >"$tmpdir/${name}.zmin.verify.out" 2>"$tmpdir/${name}.zmin.verify.err"
  compare_files verify-stdout "$tmpdir/${name}.git.verify.out" "$tmpdir/${name}.zmin.verify.out"
  compare_files verify-stderr "$tmpdir/${name}.git.verify.err" "$tmpdir/${name}.zmin.verify.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_object_dir_exact multi_pack_index_object_dir_write
