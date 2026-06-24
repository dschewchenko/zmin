#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-merge-file-schema-oracle.XXXXXX")"
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

seed_files() {
  local repo="$1"
  mkdir "$repo"
  printf 'one\nours\nthree\n' >"$repo/ours.txt"
  printf 'one\nbase\nthree\n' >"$repo/base.txt"
  printf 'one\nbase\nthree\n' >"$repo/same-base.txt"
  printf 'one\ntheirs\nthree\n' >"$repo/theirs.txt"
}

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0

  seed_files "$git_work"
  seed_files "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  compare_files ours "$git_work/ours.txt" "$zmin_work/ours.txt"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case merge_file_stdout_long merge-file --stdout ours.txt base.txt same-base.txt
run_case merge_file_quiet_conflict merge-file -q ours.txt base.txt theirs.txt
run_case merge_file_quiet_long_conflict merge-file --quiet ours.txt base.txt theirs.txt
run_case merge_file_repeated_quiet_conflict merge-file -q -q ours.txt base.txt theirs.txt
run_case merge_file_no_quiet_conflict merge-file --no-quiet ours.txt base.txt theirs.txt
run_case merge_file_quiet_short_value_invalid merge-file -q=true ours.txt base.txt theirs.txt
run_case merge_file_quiet_long_value_invalid merge-file --quiet=true ours.txt base.txt theirs.txt
run_case merge_file_ours_conflict merge-file --ours ours.txt base.txt theirs.txt
run_case merge_file_theirs_conflict merge-file --theirs ours.txt base.txt theirs.txt
run_case merge_file_union_conflict merge-file --union ours.txt base.txt theirs.txt
run_case merge_file_no_ours_conflict merge-file --no-ours ours.txt base.txt theirs.txt
run_case merge_file_no_theirs_conflict merge-file --no-theirs ours.txt base.txt theirs.txt
run_case merge_file_no_union_conflict merge-file --no-union ours.txt base.txt theirs.txt
run_case merge_file_ours_stdout_conflict merge-file -p --ours ours.txt base.txt theirs.txt
run_case merge_file_theirs_stdout_conflict merge-file -p --theirs ours.txt base.txt theirs.txt
run_case merge_file_union_stdout_conflict merge-file -p --union ours.txt base.txt theirs.txt
run_case merge_file_ours_value_invalid merge-file --ours=true ours.txt base.txt theirs.txt
run_case merge_file_theirs_value_invalid merge-file --theirs=true ours.txt base.txt theirs.txt
run_case merge_file_union_value_invalid merge-file --union=true ours.txt base.txt theirs.txt
run_case merge_file_diff3_conflict merge-file --diff3 ours.txt base.txt theirs.txt
run_case merge_file_zdiff3_conflict merge-file --zdiff3 ours.txt base.txt theirs.txt
run_case merge_file_no_diff3_conflict merge-file --no-diff3 ours.txt base.txt theirs.txt
run_case merge_file_diff3_stdout_conflict merge-file -p --diff3 ours.txt base.txt theirs.txt
run_case merge_file_zdiff3_stdout_conflict merge-file -p --zdiff3 ours.txt base.txt theirs.txt
run_case merge_file_diff3_value_invalid merge-file --diff3=true ours.txt base.txt theirs.txt
run_case merge_file_zdiff3_value_invalid merge-file --zdiff3=true ours.txt base.txt theirs.txt
run_case merge_file_marker_size_conflict merge-file --marker-size 3 ours.txt base.txt theirs.txt
run_case merge_file_marker_size_equals_conflict merge-file --marker-size=3 ours.txt base.txt theirs.txt
run_case merge_file_marker_size_stdout_conflict merge-file -p --marker-size=3 ours.txt base.txt theirs.txt
run_case merge_file_marker_size_diff3_conflict merge-file --diff3 --marker-size=3 ours.txt base.txt theirs.txt
run_case merge_file_marker_size_no_marker_conflict merge-file --no-marker-size ours.txt base.txt theirs.txt
run_case merge_file_marker_size_bogus_invalid merge-file --marker-size=bogus ours.txt base.txt theirs.txt
run_case merge_file_marker_size_missing_invalid merge-file --marker-size ours.txt base.txt theirs.txt
