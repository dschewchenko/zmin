#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-imap-send-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

seed_pair() {
  local name="$1"
  git_work="$tmpdir/${name}.git"
  zmin_work="$tmpdir/${name}.zmin"

  "$GIT_BIN" init -q -b main "$git_work"
  "$GIT_BIN" init -q -b main "$zmin_work"
}

repo_state() {
  local repo="$1"
  {
    "$GIT_BIN" -C "$repo" status --short
    find "$repo/.git" -type f | sed "s#$repo/.git/##" | LC_ALL=C sort
  }
}

run_gap() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0
  local stdout_match=0
  local stderr_match=0
  local state_match=0

  seed_pair "$name"
  repo_state "$git_work" >"$tmpdir/${name}.git.before"
  repo_state "$zmin_work" >"$tmpdir/${name}.zmin.before"

  set +e
  "$GIT_BIN" -C "$git_work" imap-send "$@" </dev/null >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" imap-send "$@") </dev/null >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  repo_state "$git_work" >"$tmpdir/${name}.git.after"
  repo_state "$zmin_work" >"$tmpdir/${name}.zmin.after"

  cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" && stdout_match=1
  cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err" && stderr_match=1
  cmp -s "$tmpdir/${name}.git.before" "$tmpdir/${name}.git.after" &&
    cmp -s "$tmpdir/${name}.zmin.before" "$tmpdir/${name}.zmin.after" &&
    state_match=1

  if [ "$git_exit" = "$zmin_exit" ] &&
    [ "$stdout_match" = 1 ] &&
    [ "$stderr_match" = 1 ] &&
    [ "$state_match" = 1 ]; then
    echo "$name unexpectedly matches stock Git; update the matrix row" >&2
    return 1
  fi

  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\tstdout_match=%s\tstderr_match=%s\tstate_match=%s\n' \
    "$name" "$git_exit" "$zmin_exit" "$stdout_match" "$stderr_match" "$state_match"
}

run_gap imap_send_curl_long --curl
run_gap imap_send_no_curl_long --no-curl
run_gap imap_send_quiet_long --quiet
run_gap imap_send_verbose_long --verbose
run_gap imap_send_quiet_short -q
run_gap imap_send_verbose_short -v
