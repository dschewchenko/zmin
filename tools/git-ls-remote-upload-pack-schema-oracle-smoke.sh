#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-ls-remote-upload-pack.XXXXXX")"
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

chmod_executable() {
  chmod +x "$1"
}

make_remote() {
  local work="$tmpdir/work"
  local remote="$tmpdir/remote.git"
  "$GIT_BIN" init -q -b main "$work"
  "$GIT_BIN" -C "$work" config user.name "Oracle"
  "$GIT_BIN" -C "$work" config user.email "oracle@example.com"
  printf 'main\n' >"$work/a.txt"
  "$GIT_BIN" -C "$work" add -A
  "$GIT_BIN" -C "$work" commit -qm "main"
  "$GIT_BIN" -C "$work" branch feature
  "$GIT_BIN" -C "$work" tag -a v1 -m "v1"
  "$GIT_BIN" init -q --bare "$remote"
  "$GIT_BIN" -C "$work" remote add origin "$remote"
  "$GIT_BIN" -C "$work" push -q origin main feature --tags
  printf '%s\n' "$remote"
}

run_case() {
  local name="$1"
  local equals_form="$2"
  local remote="$3"
  local wrapper="$tmpdir/${name}.upload-pack.sh"
  local log="$wrapper.log"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_exit=0
  local zmin_exit=0

  cat >"$wrapper" <<'SH'
#!/bin/sh
printf 'invoked %s\n' "$*" >> "$0.log"
exec git-upload-pack "$@"
SH
  chmod_executable "$wrapper"

  set +e
  if [ "$equals_form" = "1" ]; then
    "$GIT_BIN" ls-remote "--upload-pack=$wrapper" "$remote" >"$git_out" 2>"$git_err"
    git_exit=$?
    "$ZMIN_BIN" ls-remote "--upload-pack=$wrapper" "$remote" >"$zmin_out" 2>"$zmin_err"
    zmin_exit=$?
  else
    "$GIT_BIN" ls-remote --upload-pack "$wrapper" "$remote" >"$git_out" 2>"$git_err"
    git_exit=$?
    "$ZMIN_BIN" ls-remote --upload-pack "$wrapper" "$remote" >"$zmin_out" 2>"$zmin_err"
    zmin_exit=$?
  fi
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  test "$(wc -l <"$log" | tr -d ' ')" = "2"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

remote="$(make_remote)"
remote_url="file://$remote"
run_case ls_remote_upload_pack_separate 0 "$remote_url"
run_case ls_remote_upload_pack_equals 1 "$remote_url"
