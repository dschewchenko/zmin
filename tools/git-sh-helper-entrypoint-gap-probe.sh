#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-sh-helper-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

run_probe() {
  local command="$1"
  local arg="$2"
  local name="$3"
  local git_exit=0
  local zmin_exit=0

  set +e
  (
    cd "$tmpdir"
    "$GIT_BIN" "$command" "$arg"
  ) >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (
    cd "$tmpdir"
    "$ZMIN_BIN" "$command" "$arg"
  ) >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 1
  test "$zmin_exit" = 1
  cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_probe sh-i18n ignored sh_i18n_positional_helper_entrypoint_gap
run_probe sh-setup ignored sh_setup_positional_helper_entrypoint_gap
