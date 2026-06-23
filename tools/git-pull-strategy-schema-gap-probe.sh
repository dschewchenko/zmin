#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-pull-strategy-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

commit_fixed() {
  local repo="$1"
  local message="$2"
  GIT_AUTHOR_NAME="Oracle" \
    GIT_AUTHOR_EMAIL="oracle@example.test" \
    GIT_AUTHOR_DATE="2030-01-01T00:00:00 +0000" \
    GIT_COMMITTER_NAME="Oracle" \
    GIT_COMMITTER_EMAIL="oracle@example.test" \
    GIT_COMMITTER_DATE="2030-01-01T00:00:00 +0000" \
    "$GIT_BIN" -C "$repo" commit -qm "$message"
}

seed_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.test"
  printf 'base\n' >"$repo/base.txt"
  "$GIT_BIN" -C "$repo" add -A
  commit_fixed "$repo" base
  "$GIT_BIN" -C "$repo" switch -q -c side
  printf 'side\n' >"$repo/side.txt"
  "$GIT_BIN" -C "$repo" add -A
  commit_fixed "$repo" side
  "$GIT_BIN" -C "$repo" switch -q main
  printf 'main\n' >"$repo/main.txt"
  "$GIT_BIN" -C "$repo" add -A
  commit_fixed "$repo" main
}

tree_names() {
  local repo="$1"
  "$GIT_BIN" -C "$repo" ls-tree --name-only HEAD | LC_ALL=C sort
}

parent_count() {
  local repo="$1"
  "$GIT_BIN" -C "$repo" rev-list --parents -1 HEAD | awk '{ print NF }'
}

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

run_case() {
  local name="pull_strategy_long"
  local git_repo="$tmpdir/$name.git"
  local zmin_repo="$tmpdir/$name.zmin"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$git_repo"
  seed_repo "$zmin_repo"

  set +e
  "$GIT_BIN" -C "$git_repo" pull --strategy ours --no-rebase . side >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  (cd "$zmin_repo" && "$ZMIN_BIN" pull --strategy ours --no-rebase . side) >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  tree_names "$git_repo" >"$tmpdir/$name.git.tree-names"
  tree_names "$zmin_repo" >"$tmpdir/$name.zmin.tree-names"
  parent_count "$git_repo" >"$tmpdir/$name.git.parent-count"
  parent_count "$zmin_repo" >"$tmpdir/$name.zmin.parent-count"

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out"
  compare_files stderr "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"
  compare_files tree_names "$tmpdir/$name.git.tree-names" "$tmpdir/$name.zmin.tree-names"
  compare_files parent_count "$tmpdir/$name.git.parent-count" "$tmpdir/$name.zmin.parent-count"

  printf '%s\texact\texit=%s\n' "$name" "$git_exit"
}

run_case
