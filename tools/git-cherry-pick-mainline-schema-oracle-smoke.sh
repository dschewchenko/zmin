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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-cherry-pick-mainline-oracle.XXXXXX")"
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

make_merge_seed_repo() {
  local repo="$1"
  local main_parent_file="$2"
  local merge_commit_file="$3"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  "$GIT_BIN" -C "$repo" config commit.gpgsign false

  printf 'base\n' >"$repo/base.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" checkout -q -b side
  printf 'side\n' >"$repo/side.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m side
  "$GIT_BIN" -C "$repo" checkout -q main
  printf 'main\n' >"$repo/main.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m main
  "$GIT_BIN" -C "$repo" rev-parse HEAD >"$main_parent_file"
  "$GIT_BIN" -C "$repo" merge -q --no-ff side -m "merge side"
  "$GIT_BIN" -C "$repo" rev-parse HEAD >"$merge_commit_file"
}

record_clean_state() {
  local repo="$1"
  local prefix="$2"
  "$GIT_BIN" -C "$repo" rev-parse HEAD >"$prefix.head"
  "$GIT_BIN" -C "$repo" cat-file -p HEAD >"$prefix.commit"
  "$GIT_BIN" -C "$repo" cat-file -p HEAD^{tree} >"$prefix.tree"
  "$GIT_BIN" -C "$repo" status --short >"$prefix.status"
  "$GIT_BIN" -C "$repo" ls-files -s >"$prefix.index"
  for name in CHERRY_PICK_HEAD MERGE_MSG AUTO_MERGE COMMIT_EDITMSG MERGE_HEAD ORIG_HEAD; do
    if test -e "$repo/.git/$name"; then
      printf '%s\n' "$name"
    fi
  done | sort >"$prefix.git-side-effects"
}

run_mainline_gap() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local main_parent_file="$tmpdir/${name}.main-parent"
  local merge_commit_file="$tmpdir/${name}.merge-commit"
  local git_exit=0
  local zmin_exit=0

  make_merge_seed_repo "$seed" "$main_parent_file" "$merge_commit_file"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"
  "$GIT_BIN" -C "$git_work" checkout -q -B pick-mainline "$(cat "$main_parent_file")"
  "$GIT_BIN" -C "$zmin_work" checkout -q -B pick-mainline "$(cat "$main_parent_file")"

  set +e
  "$GIT_BIN" -C "$git_work" cherry-pick "$@" "$(cat "$merge_commit_file")" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" cherry-pick "$@" "$(cat "$merge_commit_file")" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if test "$git_exit" = "$zmin_exit" &&
    cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" &&
    cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"; then
    echo "$name unexpectedly matches stock Git; update the open matrix row" >&2
    return 1
  fi

  record_clean_state "$git_work" "$tmpdir/${name}.git"
  record_clean_state "$zmin_work" "$tmpdir/${name}.zmin"
  compare_files head "$tmpdir/${name}.git.head" "$tmpdir/${name}.zmin.head"
  compare_files commit "$tmpdir/${name}.git.commit" "$tmpdir/${name}.zmin.commit"
  compare_files tree "$tmpdir/${name}.git.tree" "$tmpdir/${name}.zmin.tree"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  compare_files index "$tmpdir/${name}.git.index" "$tmpdir/${name}.zmin.index"
  printf '%s\tgap\tgit_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_mainline_gap cherry_pick_mainline_long --mainline 1
