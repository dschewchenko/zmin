#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-pull-upload-pack.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

chmod_executable() {
  chmod +x "$1"
}

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

make_source() {
  local source="$1"
  "$GIT_BIN" init -q -b main "$source"
  "$GIT_BIN" -C "$source" config user.name "Oracle"
  "$GIT_BIN" -C "$source" config user.email "oracle@example.com"
  printf 'base\n' >"$source/README.md"
  "$GIT_BIN" -C "$source" add README.md
  "$GIT_BIN" -C "$source" commit -qm "base"
}

advance_source() {
  local source="$1"
  printf 'next\n' >"$source/README.md"
  printf 'added\n' >"$source/next.txt"
  "$GIT_BIN" -C "$source" add README.md next.txt
  "$GIT_BIN" -C "$source" commit -qm "next"
}

compare_repo_state() {
  local git_client="$1"
  local zmin_client="$2"

  test "$("$GIT_BIN" -C "$zmin_client" rev-parse HEAD)" = "$("$GIT_BIN" -C "$git_client" rev-parse HEAD)"
  test "$("$GIT_BIN" -C "$zmin_client" cat-file -p HEAD^{tree})" = "$("$GIT_BIN" -C "$git_client" cat-file -p HEAD^{tree})"
  test "$("$GIT_BIN" -C "$zmin_client" status --porcelain=v1 --branch)" = "$("$GIT_BIN" -C "$git_client" status --porcelain=v1 --branch)"
  cmp -s "$zmin_client/.git/FETCH_HEAD" "$git_client/.git/FETCH_HEAD"
}

run_case() {
  local name="$1"
  local equals_form="$2"
  local root="$tmpdir/$name"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  local wrapper="$root/upload-pack.sh"
  local log="$wrapper.log"
  local git_exit=0
  local zmin_exit=0

  mkdir -p "$root"
  make_source "$source"
  "$GIT_BIN" clone -q "$source" "$git_client"
  "$GIT_BIN" clone -q "$source" "$zmin_client"
  advance_source "$source"

  cat >"$wrapper" <<'SH'
#!/bin/sh
printf 'invoked %s\n' "$*" >> "$0.log"
exec git-upload-pack "$@"
SH
  chmod_executable "$wrapper"

  local wrapper_command="$wrapper"
  set +e
  if [ "$equals_form" = "1" ]; then
    "$GIT_BIN" -C "$git_client" pull --ff-only "--upload-pack=$wrapper_command" origin main >"$root/git.out" 2>"$root/git.err"
    git_exit=$?
    "$ZMIN_BIN" -C "$zmin_client" pull --ff-only "--upload-pack=$wrapper_command" origin main >"$root/zmin.out" 2>"$root/zmin.err"
    zmin_exit=$?
  else
    "$GIT_BIN" -C "$git_client" pull --ff-only --upload-pack "$wrapper_command" origin main >"$root/git.out" 2>"$root/git.err"
    git_exit=$?
    "$ZMIN_BIN" -C "$zmin_client" pull --ff-only --upload-pack "$wrapper_command" origin main >"$root/zmin.out" 2>"$root/zmin.err"
    zmin_exit=$?
  fi
  set -e

  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  normalize_err "$root" "$root/git.err" "$root/git.err.norm"
  normalize_err "$root" "$root/zmin.err" "$root/zmin.err.norm"
  cmp -s "$root/git.err.norm" "$root/zmin.err.norm"
  compare_repo_state "$git_client" "$zmin_client"
  test "$(wc -l <"$log" | tr -d ' ')" = "2"
  printf '%s\texact\texit=%s\n' "$name" "$git_exit"
}

run_case pull_upload_pack_separate 0
run_case pull_upload_pack_equals 1
