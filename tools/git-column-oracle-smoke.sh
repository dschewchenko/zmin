#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-column-oracle.XXXXXX")"
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

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
}

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_exit=0
  local zmin_exit=0

  cp -R "$base_seed" "$git_work"
  cp -R "$base_seed" "$zmin_work"

  set +e
  printf 'alpha\nbeta\ngamma\ndelta\n' | (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  printf 'alpha\nbeta\ngamma\ndelta\n' | (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

base_seed="$tmpdir/base"
make_seed_repo "$base_seed"

run_case column_padding_width column --mode=column --padding=2 --width=20
run_case column_width column --width=20
run_case column_raw_mode column --raw-mode=16 --width=20
run_case column_command_status column --command=status --width=20
run_case column_indent column --mode=column --indent='>>' --width=20
run_case column_nl column --mode=column --nl=ZZ --width=20
run_case column_raw_mode_zero column --raw-mode=0 --width=20
run_case column_no_mode column --no-mode --width=20
run_case column_mode_missing_value column --mode --width=20
run_case column_mode_empty_value column --mode= --width=20
run_case column_no_width column --width=20 --no-width
run_case column_no_command column --no-command --width=20
run_case column_no_indent column --indent='>>' --no-indent --mode=column --width=20
run_case column_no_nl column --nl=ZZ --no-nl --mode=column --width=20
run_case column_no_padding column --padding=2 --mode=column --width=20 --no-padding
run_case column_raw_mode_one column --raw-mode=1 --width=20
run_case column_raw_mode_seventeen column --raw-mode=17 --width=20
run_case column_raw_mode_zero_then_mode column --raw-mode=0 --mode=column --width=20
run_case column_mode_then_raw_mode_zero column --mode=column --raw-mode=0 --width=20
run_case column_raw_mode_one_then_mode column --raw-mode=1 --mode=column --width=20
run_case column_mode_then_raw_mode_one column --mode=column --raw-mode=1 --width=20
run_case column_raw_mode_one_padding column --raw-mode=1 --padding=2 --width=20
run_case column_raw_mode_one_indent column --raw-mode=1 --indent='>>' --width=20
run_case column_raw_mode_one_nl column --raw-mode=1 --nl=ZZ --width=20

run_failure_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_exit=0
  local zmin_exit=0

  cp -R "$base_seed" "$git_work"
  cp -R "$base_seed" "$zmin_work"

  set +e
  printf 'alpha\nbeta\ngamma\ndelta\n' | (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  printf 'alpha\nbeta\ngamma\ndelta\n' | (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_failure_case column_width_empty column --width=
run_failure_case column_padding_empty column --padding=
run_failure_case column_raw_mode_invalid column --raw-mode=bogus --width=20
