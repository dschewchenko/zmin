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
  git_remote="$tmpdir/${name}.git.remote.git"
  zmin_remote="$tmpdir/${name}.zmin.remote.git"
  git_work="$tmpdir/${name}.git"
  zmin_work="$tmpdir/${name}.zmin"

  "$GIT_BIN" init -q -b main "$source"
  "$GIT_BIN" -C "$source" config user.name "Oracle"
  "$GIT_BIN" -C "$source" config user.email "oracle@example.test"
  printf 'one\n' >"$source/a.txt"
  "$GIT_BIN" -C "$source" add -A
  commit_fixed "$source" "one"
  "$GIT_BIN" clone -q --bare "$source" "$git_remote"
  "$GIT_BIN" clone -q --bare "$source" "$zmin_remote"
  "$GIT_BIN" clone -q "$source" "$git_work"
  "$GIT_BIN" clone -q "$source" "$zmin_work"
  "$GIT_BIN" -C "$git_work" remote set-url origin "$git_remote"
  "$GIT_BIN" -C "$zmin_work" remote set-url origin "$zmin_remote"
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
    rewrite_remote "$git_remote"
    rewrite_remote "$zmin_remote"
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
    "$GIT_BIN" -C "$repo" commit -qm "$message"
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
    "$GIT_BIN" -C "$work" commit -am "remote" -q
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
  (cd "$zmin_work" && "$ZMIN_BIN" push "$@") >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  remote_refs "$git_remote" >"$tmpdir/${name}.git.refs"
  remote_refs "$zmin_remote" >"$tmpdir/${name}.zmin.refs"
  upstream_config "$git_work" >"$tmpdir/${name}.git.config"
  upstream_config "$zmin_work" >"$tmpdir/${name}.zmin.config"
  cmp -s "$tmpdir/${name}.git.refs" "$tmpdir/${name}.zmin.refs" && refs_match=1
  cmp -s "$tmpdir/${name}.git.config" "$tmpdir/${name}.zmin.config" && config_match=1
  cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" && stdout_match=1
  cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err" && stderr_match=1
  if [ "$git_exit" = "$zmin_exit" ] &&
    [ "$refs_match" = 1 ] &&
    [ "$config_match" = 1 ] &&
    [ "$stdout_match" = 1 ] &&
    [ "$stderr_match" = 1 ]; then
    echo "$name unexpectedly matches stock Git; update the matrix row" >&2
    return 1
  fi
  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\trefs_match=%s\tconfig_match=%s\tstdout_match=%s\tstderr_match=%s\n' \
    "$name" "$git_exit" "$zmin_exit" "$refs_match" "$config_match" "$stdout_match" "$stderr_match"
}

run_gap push_set_upstream_long initial --set-upstream origin main
run_gap push_force_long nonff --force origin main
run_gap push_force_short nonff -f origin main
