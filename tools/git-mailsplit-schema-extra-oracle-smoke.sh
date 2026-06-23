#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-mailsplit-oracle.XXXXXX")"
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

init_input() {
  local root="$1"
  mkdir -p "$root/out"
  printf 'From one@example.com Thu Jan  1 00:00:00 1970\r\nSubject: One\r\n\r\nBody one\r\n\r\nFrom two@example.com Thu Jan  1 00:00:00 1970\r\nSubject: Two\r\n\r\nBody two\r\n' >"$root/mbox"
}

snapshot_output() {
  local root="$1"
  local out_prefix="$2"
  find "$root/out" -maxdepth 1 -type f -print | sort | sed "s#$root/out/##" >"${out_prefix}.files"
  while IFS= read -r file; do
    cat "$root/out/$file" >"${out_prefix}.${file}.content"
  done <"${out_prefix}.files"
}

compare_output_files() {
  local name="$1"
  local git_root="$2"
  local zmin_root="$3"
  snapshot_output "$git_root" "$tmpdir/${name}.git"
  snapshot_output "$zmin_root" "$tmpdir/${name}.zmin"
  compare_files output-files "$tmpdir/${name}.git.files" "$tmpdir/${name}.zmin.files"
  while IFS= read -r file; do
    compare_files "output-$file" "$tmpdir/${name}.git.${file}.content" "$tmpdir/${name}.zmin.${file}.content"
  done <"$tmpdir/${name}.git.files"
}

run_case() {
  local name="$1"
  shift
  local git_root="$tmpdir/${name}.git"
  local zmin_root="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  init_input "$git_root"
  init_input "$zmin_root"

  set +e
  (cd "$git_root" && "$GIT_BIN" mailsplit "$@") >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (cd "$zmin_root" && "$ZMIN_BIN" mailsplit "$@") >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  compare_output_files "$name" "$git_root" "$zmin_root"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case mailsplit_keep_cr_long --keep-cr -oout mbox
run_case mailsplit_keep_from_short -b -oout mbox
