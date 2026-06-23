#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-fetch-short.XXXXXX")"
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

git_ref_snapshot() {
  local repo="$1"
  "$GIT_BIN" -C "$repo" show-ref | LC_ALL=C sort
}

fetch_head_snapshot() {
  local repo="$1"
  local path="$repo/.git/FETCH_HEAD"
  if [ -f "$path" ]; then
    cat "$path"
  fi
}

seed_remote_pair() {
  local name="$1"
  local mode="${2:-default}"
  local create_tag=1
  source_repo="$tmpdir/${name}.src"
  git_work="$tmpdir/${name}.git"
  zmin_work="$tmpdir/${name}.zmin"

  "$GIT_BIN" init -q -b main "$source_repo"
  "$GIT_BIN" -C "$source_repo" config user.name "Oracle"
  "$GIT_BIN" -C "$source_repo" config user.email "oracle@example.test"
  printf 'one\n' >"$source_repo/a.txt"
  "$GIT_BIN" -C "$source_repo" add -A
  "$GIT_BIN" -C "$source_repo" commit -qm "one"
  "$GIT_BIN" -C "$source_repo" tag v1
  "$GIT_BIN" clone -q "$source_repo" "$git_work"
  "$GIT_BIN" clone -q "$source_repo" "$zmin_work"

  case "$mode" in
    stale)
      "$GIT_BIN" -C "$source_repo" checkout -q -b old
      printf 'old\n' >"$source_repo/old.txt"
      "$GIT_BIN" -C "$source_repo" add -A
      "$GIT_BIN" -C "$source_repo" commit -qm "old"
      "$GIT_BIN" -C "$git_work" fetch -q origin
      "$GIT_BIN" -C "$zmin_work" fetch -q origin
      "$GIT_BIN" -C "$source_repo" checkout -q main
      "$GIT_BIN" -C "$source_repo" branch -D old >/dev/null
      ;;
    nonff)
      printf 'two\n' >"$source_repo/a.txt"
      "$GIT_BIN" -C "$source_repo" commit -am "two" -q
      "$GIT_BIN" -C "$git_work" fetch -q origin
      "$GIT_BIN" -C "$zmin_work" fetch -q origin
      "$GIT_BIN" -C "$source_repo" reset --hard HEAD~1 >/dev/null
      printf 'alt\n' >"$source_repo/a.txt"
      "$GIT_BIN" -C "$source_repo" commit -am "alt" -q
      return
      ;;
    append)
      printf 'old-fetch-head\n' >"$git_work/.git/FETCH_HEAD"
      printf 'old-fetch-head\n' >"$zmin_work/.git/FETCH_HEAD"
      create_tag=0
      ;;
  esac

  printf 'two\n' >"$source_repo/a.txt"
  "$GIT_BIN" -C "$source_repo" commit -am "two" -q
  if [ "$create_tag" = 1 ]; then
    "$GIT_BIN" -C "$source_repo" tag v2
  fi
}

run_fetch() {
  local name="$1"
  shift
  git_exit=0
  zmin_exit=0

  set +e
  "$GIT_BIN" -C "$git_work" fetch "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" fetch "$@") >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e
  git_ref_snapshot "$git_work" >"$tmpdir/${name}.git.refs"
  git_ref_snapshot "$zmin_work" >"$tmpdir/${name}.zmin.refs"
  fetch_head_snapshot "$git_work" >"$tmpdir/${name}.git.fetch-head"
  fetch_head_snapshot "$zmin_work" >"$tmpdir/${name}.zmin.fetch-head"
}

run_exact() {
  local name="$1"
  local mode="$2"
  shift 2
  seed_remote_pair "$name" "$mode"
  run_fetch "$name" "$@"

  compare_files exit <(printf '%s\n' "$git_exit") <(printf '%s\n' "$zmin_exit")
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  compare_files refs "$tmpdir/${name}.git.refs" "$tmpdir/${name}.zmin.refs"
  compare_files fetch-head "$tmpdir/${name}.git.fetch-head" "$tmpdir/${name}.zmin.fetch-head"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_gap() {
  local name="$1"
  local mode="$2"
  shift 2
  seed_remote_pair "$name" "$mode"
  run_fetch "$name" "$@"

  if [ "$git_exit" = "$zmin_exit" ] &&
    cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" &&
    cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err" &&
    cmp -s "$tmpdir/${name}.git.refs" "$tmpdir/${name}.zmin.refs" &&
    cmp -s "$tmpdir/${name}.git.fetch-head" "$tmpdir/${name}.zmin.fetch-head"; then
    echo "$name unexpectedly matches stock Git; update the matrix row" >&2
    return 1
  fi
  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_gap fetch_quiet_short default -q origin
run_gap fetch_prune_short stale -p origin
run_exact fetch_append_short append -a origin
run_gap fetch_dry_run_short default -n origin
run_gap fetch_tags_short default -t origin
run_gap fetch_verbose_short default -v origin
run_gap fetch_force_long nonff --force origin main:refs/remotes/origin/main
