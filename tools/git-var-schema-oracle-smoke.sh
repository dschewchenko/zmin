#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-var-schema-oracle.XXXXXX")"
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

repo="$tmpdir/repo"
"$GIT_BIN" init -q "$repo"
"$GIT_BIN" -C "$repo" config user.name Oracle
"$GIT_BIN" -C "$repo" config user.email oracle@example.com

run_case() {
  local name="$1"
  shift
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" -C "$repo" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$repo" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if [ "$git_exit" != "$zmin_exit" ]; then
    echo "$name exit differs: stock=$git_exit zmin=$zmin_exit" >&2
    return 1
  fi
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case var_list_repeated var -l -l
run_case var_list_compact_repeated var -ll
run_case var_list_rejects_value var -l=true
run_case var_list_rejects_empty_value var -l=
run_case var_no_list_rejected var --no-list
run_case var_nofork_rejected var --nofork
run_case var_i_rejected var -i
