#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

export GIT_AUTHOR_NAME="${GIT_AUTHOR_NAME:-Oracle}"
export GIT_AUTHOR_EMAIL="${GIT_AUTHOR_EMAIL:-oracle@example.com}"
export GIT_AUTHOR_DATE="${GIT_AUTHOR_DATE:-1700000000 +0000}"
export GIT_COMMITTER_NAME="${GIT_COMMITTER_NAME:-Oracle}"
export GIT_COMMITTER_EMAIL="${GIT_COMMITTER_EMAIL:-oracle@example.com}"
export GIT_COMMITTER_DATE="${GIT_COMMITTER_DATE:-1700000000 +0000}"

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-archive-schema-oracle.XXXXXX")"
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
  mkdir "$repo/dir"
  printf 'one\n' >"$repo/dir/one.txt"
  printf 'root\n' >"$repo/root.txt"
  "$GIT_BIN" -C "$repo" add dir/one.txt root.txt
  "$GIT_BIN" -C "$repo" commit -qm base
}

run_case() {
  local name="$1"
  local compare_archive="$2"
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

  make_seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  (cd "$git_work" && "$GIT_BIN" archive "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" archive "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files status "$git_status" "$zmin_status"
  if [ "$compare_archive" = "tar-list" ]; then
    tar -tf "$git_work/out.tar" >"$tmpdir/${name}.git.tarlist"
    tar -tf "$zmin_work/out.tar" >"$tmpdir/${name}.zmin.tarlist"
    compare_files tar-list "$tmpdir/${name}.git.tarlist" "$tmpdir/${name}.zmin.tarlist"
  fi
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case archive_list_short none -l
run_case archive_verbose_short tar-list --format=tar -v --output=out.tar HEAD dir
