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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-name-rev-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'base\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add file.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" tag v1.0.0
  printf 'main\n' >>"$repo/file.txt"
  "$GIT_BIN" -C "$repo" commit -q -am main
  "$GIT_BIN" -C "$repo" branch feature HEAD~1
  "$GIT_BIN" -C "$repo" checkout -q feature
  printf 'feature\n' >>"$repo/file.txt"
  "$GIT_BIN" -C "$repo" commit -q -am feature
  "$GIT_BIN" -C "$repo" checkout -q main
  "$GIT_BIN" -C "$repo" update-ref refs/remotes/origin/main refs/heads/main
}

run_gap() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0
  seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  printf 'stock stdout:\n'
  cat "$tmpdir/$name.git.out"
  printf 'zmin stdout:\n'
  cat "$tmpdir/$name.zmin.out"

  test "$git_exit" = "$zmin_exit"
  cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"
  if cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out"; then
    echo "$name unexpectedly matched" >&2
    return 1
  fi
}

run_gap name_rev_all name-rev --all
