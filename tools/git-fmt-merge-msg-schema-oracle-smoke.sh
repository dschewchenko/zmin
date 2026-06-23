#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-fmt-merge-msg-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

fetch_head="$tmpdir/FETCH_HEAD"
printf "%s\tnot-for-merge\tbranch 'feature' of ../remote\n" \
  "0123456789012345678901234567890123456789" >"$fetch_head"

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

run_stdin_case() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" fmt-merge-msg "$@" <"$fetch_head" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" fmt-merge-msg "$@" <"$fetch_head" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_file_case() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" fmt-merge-msg "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" fmt-merge-msg "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_stdin_gap() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" fmt-merge-msg "$@" <"$fetch_head" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" fmt-merge-msg "$@" <"$fetch_head" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if test "$git_exit" = "$zmin_exit" &&
    cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" &&
    cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"; then
    echo "$name unexpectedly matches stock Git; update the open matrix row" >&2
    return 1
  fi

  printf '%s\tgap\tgit_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_file_case fmt_merge_msg_file_long --file "$fetch_head"
run_stdin_case fmt_merge_msg_log_long --log
run_stdin_case fmt_merge_msg_no_log_long --no-log
run_stdin_gap fmt_merge_msg_message_long --message "Custom merge"
