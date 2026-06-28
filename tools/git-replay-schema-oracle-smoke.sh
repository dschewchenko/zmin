#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/debug/zmin}"
GIT_BIN="${GIT_BIN:-git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-replay-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

seed_source_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name Bench
  "$GIT_BIN" -C "$repo" config user.email bench@example.test
  "$GIT_BIN" -C "$repo" config commit.gpgsign false
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add -A
  GIT_AUTHOR_DATE='1700000000 +0000' GIT_COMMITTER_DATE='1700000000 +0000' \
    "$GIT_BIN" -C "$repo" commit -qm one
  base="$("$GIT_BIN" -C "$repo" rev-parse HEAD)"
  printf 'two\n' >"$repo/a.txt"
  GIT_AUTHOR_DATE='1700000010 +0000' GIT_COMMITTER_DATE='1700000010 +0000' \
    "$GIT_BIN" -C "$repo" commit -qam two
  tip="$("$GIT_BIN" -C "$repo" rev-parse HEAD)"
  range="$base..$tip"
}

prepare_case_repo() {
  local source="$1"
  local repo="$2"
  cp -R "$source" "$repo"
  "$GIT_BIN" -C "$repo" branch topic "$base"
}

run_case() {
  local name="$1"
  local mode="$2"
  shift 2
  local git_repo="$tmpdir/$name.git"
  local zmin_repo="$tmpdir/$name.zmin"
  prepare_case_repo "$source" "$git_repo"
  prepare_case_repo "$source" "$zmin_repo"

  local git_exit=0
  local zmin_exit=0
  set +e
  if [ "$mode" = "stdin" ]; then
    printf '%s\n' "$range" | GIT_EDITOR=true GIT_COMMITTER_DATE='1700000100 +0000' \
      "$GIT_BIN" -C "$git_repo" "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
    git_exit=$?
    (
      cd "$zmin_repo"
      printf '%s\n' "$range" | GIT_EDITOR=true GIT_COMMITTER_DATE='1700000100 +0000' \
        "$ZMIN_BIN" "$@"
    ) >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
    zmin_exit=$?
  else
    GIT_EDITOR=true GIT_COMMITTER_DATE='1700000100 +0000' \
      "$GIT_BIN" -C "$git_repo" "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
    git_exit=$?
    (
      cd "$zmin_repo"
      GIT_EDITOR=true GIT_COMMITTER_DATE='1700000100 +0000' \
        "$ZMIN_BIN" "$@"
    ) >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
    zmin_exit=$?
  fi
  set -e

  "$GIT_BIN" -C "$git_repo" show-ref --hash refs/heads/topic >"$tmpdir/$name.git.topic"
  "$GIT_BIN" -C "$zmin_repo" show-ref --hash refs/heads/topic >"$tmpdir/$name.zmin.topic"
  "$GIT_BIN" -C "$git_repo" status --short >"$tmpdir/$name.git.status"
  "$GIT_BIN" -C "$zmin_repo" status --short >"$tmpdir/$name.zmin.status"

  local stdout_match=0
  local stderr_match=0
  local topic_match=0
  local status_match=0
  cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out" && stdout_match=1
  cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err" && stderr_match=1
  cmp -s "$tmpdir/$name.git.topic" "$tmpdir/$name.zmin.topic" && topic_match=1
  cmp -s "$tmpdir/$name.git.status" "$tmpdir/$name.zmin.status" && status_match=1

  if [ "$git_exit" = "$zmin_exit" ] &&
    [ "$stdout_match" = 1 ] &&
    [ "$stderr_match" = 1 ] &&
    [ "$topic_match" = 1 ] &&
    [ "$status_match" = 1 ]; then
    printf '%s\texact\tstock_exit=%s\tzmin_exit=%s\tstdout_match=%s\tstderr_match=%s\ttopic_match=%s\tstatus_match=%s\n' \
      "$name" "$git_exit" "$zmin_exit" "$stdout_match" "$stderr_match" "$topic_match" "$status_match"
    return 0
  fi

  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\tstdout_match=%s\tstderr_match=%s\ttopic_match=%s\tstatus_match=%s\n' \
    "$name" "$git_exit" "$zmin_exit" "$stdout_match" "$stderr_match" "$topic_match" "$status_match"
  return 1
}

source="$tmpdir/source"
seed_source_repo "$source"

run_case replay_topo_order arg replay --topo-order --advance topic "$range"
run_case replay_date_order arg replay --date-order --advance topic "$range"
run_case replay_author_date_order arg replay --author-date-order --advance topic "$range"
run_case replay_reverse arg replay --reverse --advance topic "$range"
run_case replay_count arg replay --count --advance topic "$range"
run_case replay_tags arg replay --tags --advance topic "$range"
run_case replay_remotes arg replay --remotes --advance topic "$range"
run_case replay_branches_multiple_sources arg replay --branches --advance topic "$range"
run_case replay_all_multiple_sources arg replay --all --advance topic "$range"
run_case replay_not_empty_selection arg replay --advance topic --not "$base" "$tip"
run_case replay_stdin stdin replay --stdin --advance topic
