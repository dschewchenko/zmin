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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-cherry-pick-no-commit-oracle.XXXXXX")"
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

make_seed_repo() {
  local repo="$1"
  local oid_file="$2"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'base\n' >"$repo/base.txt"
  "$GIT_BIN" -C "$repo" add base.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" checkout -q -b feature
  printf 'feature\n' >"$repo/feature.txt"
  "$GIT_BIN" -C "$repo" add feature.txt
  "$GIT_BIN" -C "$repo" commit -q -m "feature subject"
  "$GIT_BIN" -C "$repo" rev-parse HEAD >"$oid_file"
  "$GIT_BIN" -C "$repo" checkout -q main
}

record_repo_state() {
  local repo="$1"
  local prefix="$2"
  "$GIT_BIN" -C "$repo" rev-parse HEAD >"$prefix.head"
  "$GIT_BIN" -C "$repo" status --short >"$prefix.status"
  "$GIT_BIN" -C "$repo" ls-files -s >"$prefix.index"
  "$GIT_BIN" -C "$repo" write-tree >"$prefix.tree"
  find "$repo/.git" -maxdepth 1 -type f -exec basename {} \; | sort >"$prefix.git-files"
}

compare_side_effect_file() {
  local name="$1"
  local git_work="$2"
  local zmin_work="$3"
  local git_path="$git_work/.git/$name"
  local zmin_path="$zmin_work/.git/$name"
  test -e "$git_path"
  test -e "$zmin_path"
  compare_files "$name" "$git_path" "$zmin_path"
}

compare_absent_file() {
  local name="$1"
  local git_work="$2"
  local zmin_work="$3"
  test ! -e "$git_work/.git/$name"
  test ! -e "$zmin_work/.git/$name"
}

run_case() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local oid_file="$tmpdir/${name}.oid"
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$seed" "$oid_file"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" "$(cat "$oid_file")" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" "$(cat "$oid_file")" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"

  record_repo_state "$git_work" "$tmpdir/${name}.git"
  record_repo_state "$zmin_work" "$tmpdir/${name}.zmin"
  compare_files head "$tmpdir/${name}.git.head" "$tmpdir/${name}.zmin.head"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  compare_files index "$tmpdir/${name}.git.index" "$tmpdir/${name}.zmin.index"
  compare_files tree "$tmpdir/${name}.git.tree" "$tmpdir/${name}.zmin.tree"
  compare_files git-files "$tmpdir/${name}.git.git-files" "$tmpdir/${name}.zmin.git-files"

  compare_side_effect_file AUTO_MERGE "$git_work" "$zmin_work"
  compare_side_effect_file MERGE_MSG "$git_work" "$zmin_work"
  compare_side_effect_file COMMIT_EDITMSG "$git_work" "$zmin_work"
  compare_absent_file CHERRY_PICK_HEAD "$git_work" "$zmin_work"
  compare_absent_file ORIG_HEAD "$git_work" "$zmin_work"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case cherry_pick_no_commit_long cherry-pick --no-commit
run_case cherry_pick_no_commit_short cherry-pick -n
