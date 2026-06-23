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
  "$GIT_BIN" -C "$repo" config commit.gpgsign false
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

cat_replace_objects() {
  local repo="$1"
  local out="$2"
  : >"$out"
  while read -r _ object; do
    "$GIT_BIN" -C "$repo" cat-file -p "$object" >>"$out"
  done < <(list_replace_refs "$repo")
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

run_existing_replace_oracle() {
  local name="$1"
  shift
  local root="$tmpdir/$name"
  local git_repo="$root/git"
  local zmin_repo="$root/zmin"
  local git_exit=0
  local zmin_exit=0
  mkdir "$root"
  make_repo "$git_repo"
  cp -R "$git_repo" "$zmin_repo"
  local one
  local three
  one="$("$GIT_BIN" -C "$git_repo" rev-parse HEAD~2)"
  three="$("$GIT_BIN" -C "$git_repo" rev-parse HEAD)"
  "$GIT_BIN" -C "$git_repo" replace "$three" "$one"
  "$GIT_BIN" -C "$zmin_repo" replace "$three" "$one"
  local args=()
  for arg in "$@"; do
    case "$arg" in
      __REPLACED_OBJECT__)
        args+=("$three")
        ;;
      *)
        args+=("$arg")
        ;;
    esac
  done

  set +e
  "$GIT_BIN" -C "$git_repo" replace "${args[@]}" >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" replace "${args[@]}" >"$root/zmin.out" 2>"$root/zmin.err"
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

run_graft_oracle() {
  local name="$1"
  shift
  local root="$tmpdir/$name"
  local git_repo="$root/git"
  local zmin_repo="$root/zmin"
  local git_exit=0
  local zmin_exit=0
  mkdir "$root"
  make_repo "$git_repo"
  cp -R "$git_repo" "$zmin_repo"
  local two
  two="$("$GIT_BIN" -C "$git_repo" rev-parse HEAD~1)"

  set +e
  "$GIT_BIN" -C "$git_repo" replace "$@" "$two" >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" replace "$@" "$two" >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  list_replace_refs "$git_repo" >"$root/git.refs"
  list_replace_refs "$zmin_repo" >"$root/zmin.refs"
  cat_replace_objects "$git_repo" "$root/git.objects"
  cat_replace_objects "$zmin_repo" "$root/zmin.objects"
  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  test "$git_exit" = 0
  test "$zmin_exit" = 0
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err" "$root/zmin.err"
  cmp -s "$root/git.refs" "$root/zmin.refs"
  cmp -s "$root/git.objects" "$root/zmin.objects"
}

run_edit_oracle() {
  local name="$1"
  shift
  local root="$tmpdir/$name"
  local git_repo="$root/git"
  local zmin_repo="$root/zmin"
  local git_editor="$root/git-editor.sh"
  local zmin_editor="$root/zmin-editor.sh"
  local git_exit=0
  local zmin_exit=0
  mkdir "$root"
  make_repo "$git_repo"
  cp -R "$git_repo" "$zmin_repo"
  printf '%s\n' '#!/bin/sh' 'perl -0pi -e "s/three/edited/" "$1"' >"$git_editor"
  cp "$git_editor" "$zmin_editor"
  chmod +x "$git_editor" "$zmin_editor"
  local three
  three="$("$GIT_BIN" -C "$git_repo" rev-parse HEAD)"

  set +e
  GIT_EDITOR="$git_editor" "$GIT_BIN" -C "$git_repo" replace "$@" "$three" >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  GIT_EDITOR="$zmin_editor" "$ZMIN_BIN" -C "$zmin_repo" replace "$@" "$three" >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  list_replace_refs "$git_repo" >"$root/git.refs"
  list_replace_refs "$zmin_repo" >"$root/zmin.refs"
  cat_replace_objects "$git_repo" "$root/git.objects"
  cat_replace_objects "$zmin_repo" "$root/zmin.objects"
  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  test "$git_exit" = 0
  test "$zmin_exit" = 0
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err" "$root/zmin.err"
  cmp -s "$root/git.refs" "$root/zmin.refs"
  cmp -s "$root/git.objects" "$root/zmin.objects"
}

run_force_oracle replace_force_long --force
run_force_oracle replace_force_short -f
run_existing_replace_oracle replace_list_long --list '*'
run_existing_replace_oracle replace_delete_long --delete __REPLACED_OBJECT__
run_graft_oracle replace_graft_short -g
run_edit_oracle replace_edit_short -e
