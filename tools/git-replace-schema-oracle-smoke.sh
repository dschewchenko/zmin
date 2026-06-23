#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-replace-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -qm "one"
  printf 'two\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" commit -am "two" -q
  printf 'three\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" commit -am "three" -q
}

list_replace_refs() {
  local repo="$1"
  "$GIT_BIN" -C "$repo" for-each-ref --format='%(refname) %(objectname)' refs/replace | sort
}

run_force_oracle() {
  local name="$1"
  local option="$2"
  local root="$tmpdir/$name"
  local git_repo="$root/git"
  local zmin_repo="$root/zmin"
  local git_exit=0
  local zmin_exit=0
  mkdir "$root"
  make_repo "$git_repo"
  cp -R "$git_repo" "$zmin_repo"
  one="$("$GIT_BIN" -C "$git_repo" rev-parse HEAD~2)"
  two="$("$GIT_BIN" -C "$git_repo" rev-parse HEAD~1)"
  three="$("$GIT_BIN" -C "$git_repo" rev-parse HEAD)"
  "$GIT_BIN" -C "$git_repo" replace "$three" "$one"
  "$GIT_BIN" -C "$zmin_repo" replace "$three" "$one"

  set +e
  "$GIT_BIN" -C "$git_repo" replace "$option" "$three" "$two" >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" replace "$option" "$three" "$two" >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  list_replace_refs "$git_repo" >"$root/git.refs"
  list_replace_refs "$zmin_repo" >"$root/zmin.refs"
  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  test "$git_exit" = 0
  test "$zmin_exit" = 0
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err" "$root/zmin.err"
  cmp -s "$root/git.refs" "$root/zmin.refs"
}

run_force_oracle replace_force_long --force
run_force_oracle replace_force_short -f
