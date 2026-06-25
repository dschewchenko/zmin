#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

export GIT_AUTHOR_NAME=Oracle
export GIT_AUTHOR_EMAIL=oracle@example.com
export GIT_AUTHOR_DATE="1700000000 +0000"
export GIT_COMMITTER_NAME=Oracle
export GIT_COMMITTER_EMAIL=oracle@example.com
export GIT_COMMITTER_DATE="1700000000 +0000"

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-show-index-object-format.XXXXXX")"
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

seed_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'hello\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -qm initial
  "$GIT_BIN" -C "$repo" repack -adq
}

run_case() {
  local name="$1"
  shift
  local repo="$tmpdir/${name}.repo"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$repo"
  local idx
  idx="$(find "$repo/.git/objects/pack" -name '*.idx' | head -n 1)"

  set +e
  "$GIT_BIN" -C "$repo" "$@" <"$idx" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$repo" "$@" <"$idx" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
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

run_case show_index_object_format_sha1_equals show-index --object-format=sha1
run_case show_index_object_format_sha1_separate show-index --object-format sha1
run_case show_index_object_format_repeated show-index --object-format=sha1 --object-format=sha1
run_case show_index_no_object_format show-index --no-object-format
run_case show_index_no_then_yes show-index --no-object-format --object-format=sha1
run_case show_index_yes_then_no show-index --object-format=sha1 --no-object-format
run_case show_index_sep_then_no show-index --object-format sha1 --no-object-format
run_case show_index_no_then_sep show-index --no-object-format --object-format sha1
run_case show_index_repeat_no show-index --no-object-format --no-object-format
run_case show_index_sha256 show-index --object-format=sha256
run_case show_index_sha1_then_sha256 show-index --object-format=sha1 --object-format=sha256
run_case show_index_sha256_then_sha1 show-index --object-format=sha256 --object-format=sha1
run_case show_index_no_then_sha256 show-index --no-object-format --object-format=sha256
run_case show_index_sep_then_equals show-index --object-format sha1 --object-format=sha1
run_case show_index_equals_then_sep show-index --object-format=sha1 --object-format sha1
run_case show_index_no_yes_no show-index --no-object-format --object-format=sha1 --no-object-format
run_case show_index_yes_no_yes show-index --object-format=sha1 --no-object-format --object-format=sha1
run_case show_index_sha256_then_no show-index --object-format=sha256 --no-object-format
run_case show_index_invalid_then_no show-index --object-format=bogus --no-object-format
run_case show_index_no_invalid_no show-index --no-object-format --object-format=bogus --no-object-format
run_case show_index_invalid_no_yes show-index --object-format=bogus --no-object-format --object-format=sha1
run_case show_index_sep_sep show-index --object-format sha1 --object-format sha1
run_case show_index_yes_yes_no show-index --object-format=sha1 --object-format=sha1 --no-object-format
run_case show_index_no_sep_no show-index --no-object-format --object-format sha1 --no-object-format
run_case show_index_sep_no_sep show-index --object-format sha1 --no-object-format --object-format sha1
run_case show_index_sha256_twice show-index --object-format=sha256 --object-format=sha256
run_case show_index_sha256_twice_no show-index --object-format=sha256 --object-format=sha256 --no-object-format
run_case show_index_invalid_no_no show-index --object-format=bogus --no-object-format --no-object-format
run_case show_index_no_yes_yes show-index --no-object-format --object-format=sha1 --object-format=sha1
run_case show_index_yes_no_no show-index --object-format=sha1 --no-object-format --no-object-format
run_case show_index_no_sha256_no show-index --no-object-format --object-format=sha256 --no-object-format
run_case show_index_sep_sep_no show-index --object-format sha1 --object-format sha1 --no-object-format
run_case show_index_sep_sep_no_sep show-index --object-format sha1 --object-format sha1 --no-object-format --object-format sha1
run_case show_index_no_sep_sep_no show-index --no-object-format --object-format sha1 --object-format sha1 --no-object-format
run_case show_index_yes_tripled show-index --object-format=sha1 --object-format=sha1 --object-format=sha1
run_case show_index_sha256_tripled show-index --object-format=sha256 --object-format=sha256 --object-format=sha256
run_case show_index_sha256_sha256_no_yes show-index --object-format=sha256 --object-format=sha256 --no-object-format --object-format=sha1
run_case show_index_invalid_no_yes_no show-index --object-format=bogus --no-object-format --object-format=sha1 --no-object-format
run_case show_index_no_no_yes show-index --no-object-format --no-object-format --object-format=sha1
run_case show_index_no_no_sha256 show-index --no-object-format --no-object-format --object-format=sha256
run_case show_index_yes_no_sha256_no show-index --object-format=sha1 --no-object-format --object-format=sha256 --no-object-format
run_case show_index_no_then_invalid show-index --no-object-format --object-format=bogus
run_case show_index_object_format_invalid show-index --object-format=bogus
run_case show_index_object_format_missing show-index --object-format
