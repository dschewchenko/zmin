#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-push-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

seed_pair() {
  local name="$1"
  local mode="$2"
  local source="$tmpdir/${name}.source"
  remote="$tmpdir/${name}.remote.git"
  git_work="$tmpdir/${name}.git"
  zmin_work="$tmpdir/${name}.zmin"
  initial_remote_main=""

  "$GIT_BIN" init -q -b main "$source"
  "$GIT_BIN" -C "$source" config user.name "Oracle"
  "$GIT_BIN" -C "$source" config user.email "oracle@example.test"
  printf 'one\n' >"$source/a.txt"
  "$GIT_BIN" -C "$source" add -A
  commit_fixed "$source" "one"
  "$GIT_BIN" clone -q --bare "$source" "$remote"
  "$GIT_BIN" clone -q "$source" "$git_work"
  "$GIT_BIN" clone -q "$source" "$zmin_work"
  "$GIT_BIN" -C "$git_work" remote set-url origin "$remote"
  "$GIT_BIN" -C "$zmin_work" remote set-url origin "$remote"
  "$GIT_BIN" -C "$git_work" config --unset-all branch.main.remote || true
  "$GIT_BIN" -C "$git_work" config --unset-all branch.main.merge || true
  "$GIT_BIN" -C "$zmin_work" config --unset-all branch.main.remote || true
  "$GIT_BIN" -C "$zmin_work" config --unset-all branch.main.merge || true

  if [ "$mode" = "nonff" ]; then
    for work in "$git_work" "$zmin_work"; do
      printf 'two\n' >"$work/a.txt"
      "$GIT_BIN" -C "$work" add -A
      commit_fixed "$work" "two"
    done
    rewrite_remote "$remote"
    initial_remote_main="$("$GIT_BIN" --git-dir="$remote" rev-parse refs/heads/main)"
  fi
}

commit_fixed() {
  local repo="$1"
  local message="$2"
  GIT_AUTHOR_NAME="Oracle" \
    GIT_AUTHOR_EMAIL="oracle@example.test" \
    GIT_AUTHOR_DATE="2030-01-01T00:00:00 +0000" \
    GIT_COMMITTER_NAME="Oracle" \
    GIT_COMMITTER_EMAIL="oracle@example.test" \
    GIT_COMMITTER_DATE="2030-01-01T00:00:00 +0000" \
    "$GIT_BIN" -c commit.gpgsign=false -C "$repo" commit -qm "$message"
}

rewrite_remote() {
  local remote="$1"
  local work="$tmpdir/rewrite-$(basename "$remote")"
  "$GIT_BIN" clone -q "$remote" "$work"
  "$GIT_BIN" -C "$work" config user.name "Oracle"
  "$GIT_BIN" -C "$work" config user.email "oracle@example.test"
  printf 'remote\n' >"$work/a.txt"
  GIT_AUTHOR_NAME="Oracle" \
    GIT_AUTHOR_EMAIL="oracle@example.test" \
    GIT_AUTHOR_DATE="2030-01-02T00:00:00 +0000" \
    GIT_COMMITTER_NAME="Oracle" \
    GIT_COMMITTER_EMAIL="oracle@example.test" \
    GIT_COMMITTER_DATE="2030-01-02T00:00:00 +0000" \
    "$GIT_BIN" -c commit.gpgsign=false -C "$work" commit -am "remote" -q
  "$GIT_BIN" -C "$work" push -q origin main
}

remote_refs() {
  local remote="$1"
  "$GIT_BIN" --git-dir="$remote" show-ref | sed "s#$remote#<remote>#g" | LC_ALL=C sort
}

upstream_config() {
  local repo="$1"
  "$GIT_BIN" -C "$repo" config --get-regexp '^branch\.main\.' || true
}

reset_remote_after_stock_push() {
  if [ -n "${initial_remote_main:-}" ]; then
    "$GIT_BIN" --git-dir="$remote" update-ref refs/heads/main "$initial_remote_main"
  fi
}

capture_stock_state() {
  local name="$1"
  remote_refs "$remote" >"$tmpdir/${name}.git.refs"
  upstream_config "$git_work" >"$tmpdir/${name}.git.config"
}

capture_zmin_state() {
  local name="$1"
  remote_refs "$remote" >"$tmpdir/${name}.zmin.refs"
  upstream_config "$zmin_work" >"$tmpdir/${name}.zmin.config"
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
  local name="$1"
  local mode="$2"
  shift 2
  local git_exit=0
  local zmin_exit=0

  seed_pair "$name" "$mode"

  set +e
  "$GIT_BIN" -C "$git_work" push "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  capture_stock_state "$name"
  reset_remote_after_stock_push
  (cd "$zmin_work" && "$ZMIN_BIN" push "$@") >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  capture_zmin_state "$name"

  test "$git_exit" = "$zmin_exit"
  compare_files refs "$tmpdir/${name}.git.refs" "$tmpdir/${name}.zmin.refs"
  compare_files config "$tmpdir/${name}.git.config" "$tmpdir/${name}.zmin.config"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\texact\texit=%s\n' "$name" "$git_exit"
}

run_gap() {
  local name="$1"
  local mode="$2"
  shift 2
  local git_exit=0
  local zmin_exit=0
  local refs_match=0
  local config_match=0
  local stdout_match=0
  local stderr_match=0

  seed_pair "$name" "$mode"

  set +e
  "$GIT_BIN" -C "$git_work" push "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  capture_stock_state "$name"
  reset_remote_after_stock_push
  (cd "$zmin_work" && "$ZMIN_BIN" push "$@") >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  capture_zmin_state "$name"
  cmp -s "$tmpdir/${name}.git.refs" "$tmpdir/${name}.zmin.refs" && refs_match=1
  cmp -s "$tmpdir/${name}.git.config" "$tmpdir/${name}.zmin.config" && config_match=1
  cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" && stdout_match=1
  cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err" && stderr_match=1
  if [ "$git_exit" = "$zmin_exit" ] &&
    [ "$refs_match" = 1 ] &&
    [ "$config_match" = 1 ] &&
    [ "$stdout_match" = 1 ] &&
    [ "$stderr_match" = 1 ]; then
    printf '%s\texact\tstock_exit=%s\tzmin_exit=%s\trefs_match=%s\tconfig_match=%s\tstdout_match=%s\tstderr_match=%s\n' \
      "$name" "$git_exit" "$zmin_exit" "$refs_match" "$config_match" "$stdout_match" "$stderr_match"
    return 0
  fi
  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\trefs_match=%s\tconfig_match=%s\tstdout_match=%s\tstderr_match=%s\n' \
    "$name" "$git_exit" "$zmin_exit" "$refs_match" "$config_match" "$stdout_match" "$stderr_match"
  return 1
}

run_case push_set_upstream_long initial --set-upstream origin main
run_gap push_force_long nonff --force origin main
run_gap push_force_short nonff -f origin main
