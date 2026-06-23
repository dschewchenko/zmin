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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-whatchanged-schema-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_history_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'needle\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m base
  printf 'changed\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" commit -q -am change
}

run_open_gap() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_history_repo "$git_repo"
  make_history_repo "$zmin_repo"

  set +e
  "$GIT_BIN" -C "$git_repo" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if [ "$git_exit" = "$zmin_exit" ] && cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" && cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"; then
    echo "$name unexpectedly matches stock Git; update the open matrix row" >&2
    exit 1
  fi

  case "$name" in
    whatchanged_i_still_use_this)
      test "$git_exit" != 0
      test "$zmin_exit" = 0
      ;;
    *)
      test "$git_exit" = 0
      test "$zmin_exit" != 0
      grep -q -- "--i-still-use-this" "$tmpdir/${name}.zmin.err"
      ;;
  esac

  printf '%s\topen-gap\tgit_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_open_gap whatchanged_all whatchanged --all --max-count 1
run_open_gap whatchanged_cc whatchanged --cc --max-count 1
run_open_gap whatchanged_date whatchanged --date=iso --max-count 1
run_open_gap whatchanged_format whatchanged --format=%H --max-count 1
run_open_gap whatchanged_i_still_use_this whatchanged --i-still-use-this --max-count 1
run_open_gap whatchanged_max_count whatchanged --max-count 1
run_open_gap whatchanged_name_only whatchanged --name-only --max-count 1
run_open_gap whatchanged_name_status whatchanged --name-status --max-count 1
run_open_gap whatchanged_numstat whatchanged --numstat --max-count 1
run_open_gap whatchanged_oneline whatchanged --oneline --max-count 1
run_open_gap whatchanged_parents whatchanged --parents --oneline --max-count 1
run_open_gap whatchanged_patch whatchanged --patch --max-count 1
run_open_gap whatchanged_patch_with_stat whatchanged --patch-with-stat --max-count 1
run_open_gap whatchanged_pickaxe_all whatchanged -Sneedle --pickaxe-all --name-only
run_open_gap whatchanged_pickaxe_regex whatchanged -Sneed.e --pickaxe-regex --name-only
run_open_gap whatchanged_pretty whatchanged --pretty=oneline --max-count 1
run_open_gap whatchanged_raw whatchanged --raw --max-count 1
run_open_gap whatchanged_reverse whatchanged --reverse --format=%s
run_open_gap whatchanged_root whatchanged --root --max-count 2
run_open_gap whatchanged_shortstat whatchanged --shortstat --max-count 1
run_open_gap whatchanged_since whatchanged --since=1.week.ago --format=%s
run_open_gap whatchanged_stat whatchanged --stat --max-count 1
run_open_gap whatchanged_summary whatchanged --summary --max-count 1
run_open_gap whatchanged_pickaxe_G whatchanged -Gchanged --name-only
run_open_gap whatchanged_pickaxe_S whatchanged -Sneedle --name-only
run_open_gap whatchanged_short_c whatchanged -c --max-count 1
run_open_gap whatchanged_short_n whatchanged -n 1
run_open_gap whatchanged_short_p whatchanged -p --max-count 1
run_open_gap whatchanged_revs whatchanged --oneline HEAD
