#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
export GIT_EDITOR="${GIT_EDITOR:-:}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-bugreport-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

normalize_err() {
  local outdir="$1"
  local file="$2"
  perl -0pi -e "s#'$outdir/[^']+'#'<OUTFILE>'#g" "$file"
}

run_case() {
  local name="$1"
  local output_flag="$2"
  shift 2
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_outdir="$tmpdir/${name}.git.outdir"
  local zmin_outdir="$tmpdir/${name}.zmin.outdir"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_files="$tmpdir/${name}.git.files"
  local zmin_files="$tmpdir/${name}.zmin.files"
  local git_exit=0
  local zmin_exit=0

  mkdir "$git_work" "$zmin_work" "$git_outdir" "$zmin_outdir"
  "$GIT_BIN" -C "$git_work" init -q
  "$GIT_BIN" -C "$zmin_work" init -q

  set +e
  (cd "$git_work" && "$GIT_BIN" bugreport "$output_flag" "$git_outdir" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" bugreport "$output_flag" "$zmin_outdir" "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  find "$git_outdir" -maxdepth 3 -type f | sed "s#$git_outdir/##" | sort >"$git_files"
  find "$zmin_outdir" -maxdepth 3 -type f | sed "s#$zmin_outdir/##" | sort >"$zmin_files"
  normalize_err "$git_outdir" "$git_err"
  normalize_err "$zmin_outdir" "$zmin_err"

  test "$git_exit" = "$zmin_exit"
  cmp -s "$git_out" "$zmin_out"
  cmp -s "$git_err" "$zmin_err"
  cmp -s "$git_files" "$zmin_files"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case bugreport_no_suffix_short_output -o --no-suffix
run_case bugreport_no_suffix_long_output --output-directory --no-suffix
run_case bugreport_suffix_short -o -s custom
run_case bugreport_suffix_long_equals -o --suffix=eqcustom
run_case bugreport_suffix_long_separate -o --suffix sepcustom
