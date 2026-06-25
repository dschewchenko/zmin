#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-pull-fetch-inherited.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

normalize_err() {
  local root="$1"
  local input="$2"
  local output="$3"
  sed \
    -e "s#$root/source#<source>#g" \
    -e "s#$root/git-client#<client>#g" \
    -e "s#$root/zmin-client#<client>#g" \
    "$input" >"$output"
}

compare_shallow_state() {
  local git_client="$1"
  local zmin_client="$2"
  local git_shallow="$git_client/.git/shallow"
  local zmin_shallow="$zmin_client/.git/shallow"
  if [ -f "$git_shallow" ] || [ -f "$zmin_shallow" ]; then
    test -f "$git_shallow"
    test -f "$zmin_shallow"
    cmp -s "$git_shallow" "$zmin_shallow"
  fi
}

compare_fetch_head_state() {
  local git_client="$1"
  local zmin_client="$2"
  local git_fetch_head="$git_client/.git/FETCH_HEAD"
  local zmin_fetch_head="$zmin_client/.git/FETCH_HEAD"
  if [ -f "$git_fetch_head" ] || [ -f "$zmin_fetch_head" ]; then
    test -f "$git_fetch_head"
    test -f "$zmin_fetch_head"
    cmp -s "$git_fetch_head" "$zmin_fetch_head"
  fi
}

compare_repo_state() {
  local git_client="$1"
  local zmin_client="$2"
  test "$("$GIT_BIN" -C "$zmin_client" rev-parse HEAD)" = "$("$GIT_BIN" -C "$git_client" rev-parse HEAD)"
  test "$("$GIT_BIN" -C "$zmin_client" cat-file -p HEAD^{tree})" = "$("$GIT_BIN" -C "$git_client" cat-file -p HEAD^{tree})"
  test "$("$GIT_BIN" -C "$zmin_client" status --porcelain=v1 --branch)" = "$("$GIT_BIN" -C "$git_client" status --porcelain=v1 --branch)"
  compare_fetch_head_state "$git_client" "$zmin_client"
  compare_shallow_state "$git_client" "$zmin_client"
}

configure_source() {
  local source="$1"
  "$GIT_BIN" init -q -b main "$source"
  "$GIT_BIN" -C "$source" config user.name "Oracle"
  "$GIT_BIN" -C "$source" config user.email "oracle@example.com"
}

commit_source() {
  local source="$1"
  local stamp="$2"
  local body="$3"
  printf '%s\n' "$body" >"$source/README.md"
  printf '%s\n' "$stamp" >"$source/$stamp.txt"
  "$GIT_BIN" -C "$source" add -A
  GIT_AUTHOR_DATE="2024-01-$stamp 12:00:00 +0000" \
  GIT_COMMITTER_DATE="2024-01-$stamp 12:00:00 +0000" \
    "$GIT_BIN" -C "$source" commit -qm "$body"
}

run_pull_case() {
  local name="$1"
  shift
  local root="$tmpdir/$name"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  local git_exit=0
  local zmin_exit=0

  mkdir -p "$root"
  configure_source "$source"
  "$@"

  set +e
  "$GIT_BIN" -C "$git_client" pull "$PULL_FLAGS" origin main >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_client" pull "$PULL_FLAGS" origin main >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  normalize_err "$root" "$root/git.err" "$root/git.err.norm"
  normalize_err "$root" "$root/zmin.err" "$root/zmin.err.norm"
  cmp -s "$root/git.err.norm" "$root/zmin.err.norm"
  compare_repo_state "$git_client" "$zmin_client"
  printf '%s\texact\texit=%s\n' "$name" "$git_exit"
}

setup_depth_case() {
  local source="$1"
  local git_client="$2"
  local zmin_client="$3"
  commit_source "$source" 01 base
  "$GIT_BIN" clone -q "$source" "$git_client"
  "$GIT_BIN" clone -q "$source" "$zmin_client"
  commit_source "$source" 02 next
}

setup_deepen_case() {
  local source="$1"
  local git_client="$2"
  local zmin_client="$3"
  commit_source "$source" 01 base
  commit_source "$source" 02 second
  "$GIT_BIN" clone -q --depth=1 "file://$source" "$git_client"
  "$GIT_BIN" clone -q --depth=1 "file://$source" "$zmin_client"
  commit_source "$source" 03 third
}

setup_unshallow_case() {
  local source="$1"
  local git_client="$2"
  local zmin_client="$3"
  commit_source "$source" 01 base
  commit_source "$source" 02 second
  commit_source "$source" 03 third
  "$GIT_BIN" clone -q --depth=1 "file://$source" "$git_client"
  "$GIT_BIN" clone -q --depth=1 "file://$source" "$zmin_client"
}

setup_shallow_since_case() {
  local source="$1"
  local git_client="$2"
  local zmin_client="$3"
  commit_source "$source" 01 one
  commit_source "$source" 10 two
  "$GIT_BIN" clone -q --depth=1 "file://$source" "$git_client"
  "$GIT_BIN" clone -q --depth=1 "file://$source" "$zmin_client"
  commit_source "$source" 20 three
}

setup_shallow_exclude_case() {
  local source="$1"
  local git_client="$2"
  local zmin_client="$3"
  commit_source "$source" 01 base
  local base_id
  base_id="$("$GIT_BIN" -C "$source" rev-parse HEAD)"
  printf '%s\n' "$base_id" >"$source/.base-id"
  commit_source "$source" 02 next
  "$GIT_BIN" clone -q --depth=1 "file://$source" "$git_client"
  "$GIT_BIN" clone -q --depth=1 "file://$source" "$zmin_client"
  commit_source "$source" 03 third
}

setup_update_shallow_case() {
  local source="$1"
  local git_client="$2"
  local zmin_client="$3"
  local shallow_remote
  shallow_remote="$(dirname "$git_client")/shallow.git"
  commit_source "$source" 01 one
  commit_source "$source" 02 two
  commit_source "$source" 03 three
  commit_source "$source" 04 four
  "$GIT_BIN" clone -q --bare --depth=2 "file://$source" "$shallow_remote"
  "$GIT_BIN" clone -q --depth=2 "file://$shallow_remote" "$git_client"
  "$GIT_BIN" clone -q --depth=2 "file://$shallow_remote" "$zmin_client"
  "$GIT_BIN" -C "$git_client" reset --hard -q HEAD^
  "$GIT_BIN" -C "$zmin_client" reset --hard -q HEAD^
}

run_depth_case() {
  local root="$tmpdir/pull_depth"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  mkdir -p "$root"
  configure_source "$source"
  setup_depth_case "$source" "$git_client" "$zmin_client"
  local git_exit=0
  local zmin_exit=0
  set +e
  "$GIT_BIN" -C "$git_client" pull --ff-only --depth=1 origin main >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_client" pull --ff-only --depth=1 origin main >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e
  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "128"
  cmp -s "$root/git.out" "$root/zmin.out"
  normalize_err "$root" "$root/git.err" "$root/git.err.norm"
  normalize_err "$root" "$root/zmin.err" "$root/zmin.err.norm"
  cmp -s "$root/git.err.norm" "$root/zmin.err.norm"
  compare_repo_state "$git_client" "$zmin_client"
  printf 'pull_depth\texact\texit=%s\n' "$git_exit"
}

run_deepen_case() {
  local root="$tmpdir/pull_deepen"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  mkdir -p "$root"
  configure_source "$source"
  setup_deepen_case "$source" "$git_client" "$zmin_client"
  local git_exit=0
  local zmin_exit=0
  set +e
  "$GIT_BIN" -C "$git_client" pull --ff-only --deepen=1 origin main >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_client" pull --ff-only --deepen=1 origin main >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e
  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  normalize_err "$root" "$root/git.err" "$root/git.err.norm"
  normalize_err "$root" "$root/zmin.err" "$root/zmin.err.norm"
  cmp -s "$root/git.err.norm" "$root/zmin.err.norm"
  compare_repo_state "$git_client" "$zmin_client"
  printf 'pull_deepen\texact\texit=%s\n' "$git_exit"
}

run_unshallow_case() {
  local root="$tmpdir/pull_unshallow"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  mkdir -p "$root"
  configure_source "$source"
  setup_unshallow_case "$source" "$git_client" "$zmin_client"
  local git_exit=0
  local zmin_exit=0
  set +e
  "$GIT_BIN" -C "$git_client" pull --ff-only --unshallow origin main >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_client" pull --ff-only --unshallow origin main >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e
  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  normalize_err "$root" "$root/git.err" "$root/git.err.norm"
  normalize_err "$root" "$root/zmin.err" "$root/zmin.err.norm"
  cmp -s "$root/git.err.norm" "$root/zmin.err.norm"
  compare_repo_state "$git_client" "$zmin_client"
  printf 'pull_unshallow\texact\texit=%s\n' "$git_exit"
}

run_shallow_since_case() {
  local root="$tmpdir/pull_shallow_since"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  mkdir -p "$root"
  configure_source "$source"
  setup_shallow_since_case "$source" "$git_client" "$zmin_client"
  local git_exit=0
  local zmin_exit=0
  set +e
  "$GIT_BIN" -C "$git_client" pull --ff-only --shallow-since=2024-01-05 origin main >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_client" pull --ff-only --shallow-since=2024-01-05 origin main >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e
  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  normalize_err "$root" "$root/git.err" "$root/git.err.norm"
  normalize_err "$root" "$root/zmin.err" "$root/zmin.err.norm"
  cmp -s "$root/git.err.norm" "$root/zmin.err.norm"
  compare_repo_state "$git_client" "$zmin_client"
  printf 'pull_shallow_since\texact\texit=%s\n' "$git_exit"
}

run_shallow_exclude_case() {
  local root="$tmpdir/pull_shallow_exclude"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  mkdir -p "$root"
  configure_source "$source"
  setup_shallow_exclude_case "$source" "$git_client" "$zmin_client"
  local base_id
  base_id="$(cat "$source/.base-id")"
  local git_exit=0
  local zmin_exit=0
  set +e
  "$GIT_BIN" -C "$git_client" pull --ff-only --shallow-exclude="$base_id" origin main >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_client" pull --ff-only --shallow-exclude="$base_id" origin main >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e
  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "1"
  cmp -s "$root/git.out" "$root/zmin.out"
  normalize_err "$root" "$root/git.err" "$root/git.err.norm"
  normalize_err "$root" "$root/zmin.err" "$root/zmin.err.norm"
  cmp -s "$root/git.err.norm" "$root/zmin.err.norm"
  compare_repo_state "$git_client" "$zmin_client"
  printf 'pull_shallow_exclude\tinvalid-input\texit=%s\n' "$git_exit"
}

run_update_shallow_case() {
  local root="$tmpdir/pull_update_shallow"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  mkdir -p "$root"
  configure_source "$source"
  setup_update_shallow_case "$source" "$git_client" "$zmin_client"
  local git_exit=0
  local zmin_exit=0
  set +e
  "$GIT_BIN" -C "$git_client" pull --ff-only --update-shallow origin main >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_client" pull --ff-only --update-shallow origin main >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e
  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  normalize_err "$root" "$root/git.err" "$root/git.err.norm"
  normalize_err "$root" "$root/zmin.err" "$root/zmin.err.norm"
  cmp -s "$root/git.err.norm" "$root/zmin.err.norm"
  compare_repo_state "$git_client" "$zmin_client"
  printf 'pull_update_shallow\texact\texit=%s\n' "$git_exit"
}

run_depth_case
run_deepen_case
run_unshallow_case
run_shallow_since_case
run_shallow_exclude_case
run_update_shallow_case
