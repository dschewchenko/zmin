#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-merge-tree-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf 'base\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -qm "base"
  base_commit="$("$GIT_BIN" -C "$repo" rev-parse HEAD)"
  base_tree="$("$GIT_BIN" -C "$repo" rev-parse HEAD^{tree})"

  "$GIT_BIN" -C "$repo" switch -q -c ours
  printf 'ours\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -qm "ours"
  ours_commit="$("$GIT_BIN" -C "$repo" rev-parse HEAD)"

  "$GIT_BIN" -C "$repo" switch -q --detach "$base_commit"
  "$GIT_BIN" -C "$repo" switch -q -c theirs
  printf 'theirs\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -qm "theirs"
  theirs_commit="$("$GIT_BIN" -C "$repo" rev-parse HEAD)"
  theirs_tree="$("$GIT_BIN" -C "$repo" rev-parse HEAD^{tree})"
}

run_gap() {
  local name="$1"
  local stdin_data="$2"
  shift 2
  local git_exit=0
  local zmin_exit=0

  set +e
  printf '%s' "$stdin_data" | "$GIT_BIN" -C "$repo" merge-tree "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  printf '%s' "$stdin_data" | "$ZMIN_BIN" -C "$repo" merge-tree "$@" >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  printf 'stock stdout:\n'
  sed -n '1,6p' "$tmpdir/$name.git.out"
  printf 'zmin stdout:\n'
  sed -n '1,6p' "$tmpdir/$name.zmin.out"
  printf 'stock stderr:\n'
  sed -n '1,4p' "$tmpdir/$name.git.err"
  printf 'zmin stderr:\n'
  sed -n '1,4p' "$tmpdir/$name.zmin.err"

  if [ "$git_exit" = "$zmin_exit" ] \
    && cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out" \
    && cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"; then
    echo "$name unexpectedly matched" >&2
    return 1
  fi
}

repo="$tmpdir/repo"
make_repo "$repo"

run_gap merge_tree_write_tree "" --write-tree "$ours_commit" "$theirs_commit"
run_gap merge_tree_messages "" --write-tree --messages "$ours_commit" "$theirs_commit"
run_gap merge_tree_no_messages "" --write-tree --no-messages "$ours_commit" "$theirs_commit"
run_gap merge_tree_quiet "" --write-tree --quiet "$ours_commit" "$theirs_commit"
run_gap merge_tree_z "" --write-tree -z "$ours_commit" "$theirs_commit"
run_gap merge_tree_name_only "" --write-tree --name-only "$ours_commit" "$theirs_commit"
run_gap merge_tree_allow_unrelated "" --write-tree --allow-unrelated-histories "$ours_commit" "$theirs_commit"
run_gap merge_tree_stdin "$ours_commit $theirs_commit"$'\n' --write-tree --stdin
run_gap merge_tree_merge_base "" --write-tree --merge-base="$base_commit" "$ours_commit" "$theirs_commit"
run_gap merge_tree_strategy_option "" --write-tree --strategy-option=ours "$ours_commit" "$theirs_commit"
run_gap merge_tree_strategy_option_short "" --write-tree -X ours "$ours_commit" "$theirs_commit"
