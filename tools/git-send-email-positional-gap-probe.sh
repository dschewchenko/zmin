#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-send-email-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

seed_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
}

run_gap() {
  local name="send_email_missing_patch_arg"
  local git_repo="$tmpdir/$name.git"
  local zmin_repo="$tmpdir/$name.zmin"
  local git_exit=0
  local zmin_exit=0
  local stdout_match=0
  local stderr_match=0
  local state_match=0

  seed_repo "$git_repo"
  seed_repo "$zmin_repo"

  set +e
  "$GIT_BIN" -C "$git_repo" send-email missing.patch >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  (cd "$zmin_repo" && "$ZMIN_BIN" send-email missing.patch) >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  "$GIT_BIN" -C "$git_repo" status --short >"$tmpdir/$name.git.status"
  "$GIT_BIN" -C "$zmin_repo" status --short >"$tmpdir/$name.zmin.status"
  cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out" && stdout_match=1
  cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err" && stderr_match=1
  cmp -s "$tmpdir/$name.git.status" "$tmpdir/$name.zmin.status" && state_match=1

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

run_gap
