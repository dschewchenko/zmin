#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-cat-file-oracle.XXXXXX")"
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
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf 'hello\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add file.txt
  "$GIT_BIN" -C "$repo" commit -qm "base"
}

run_case() {
  local name="$1"
  local stdin_payload="$2"
  shift 2
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
  if [[ -n "$stdin_payload" ]]; then
    printf '%b' "$stdin_payload" | (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
    git_exit=$?
    printf '%b' "$stdin_payload" | (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
    zmin_exit=$?
  else
    (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
    git_exit=$?
    (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
    zmin_exit=$?
  fi
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

base_seed="$tmpdir/base-for-oid"
make_seed_repo "$base_seed"
blob_oid="$("$GIT_BIN" -C "$base_seed" rev-parse HEAD:file.txt)"

run_case cat_file_typed_blob "" cat-file blob "$blob_oid"
run_case cat_file_batch_check_no_buffer "$blob_oid\n" cat-file --batch-check --no-buffer
run_case cat_file_batch_check_follow_symlinks "$blob_oid\n" cat-file --batch-check --follow-symlinks
run_case cat_file_batch_check_no_filter "$blob_oid\n" cat-file --batch-check --no-filter
run_case cat_file_batch_check_z "$blob_oid\n" cat-file --batch-check -z
run_case cat_file_batch_check_full_nul "$blob_oid\n" cat-file --batch-check -Z
