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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-gc-schema-oracle.XXXXXX")"
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
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m one
  printf 'two\n' >"$repo/b.txt"
  "$GIT_BIN" -C "$repo" add b.txt
  "$GIT_BIN" -C "$repo" commit -q -m two
}

object_files() {
  local repo="$1"
  find "$repo/.git/objects" -type f \
    | sed "s#$repo/.git/objects/##" \
    | sort
}

run_exact() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  object_files "$git_work" >"$tmpdir/${name}.git.objects"
  object_files "$zmin_work" >"$tmpdir/${name}.zmin.objects"
  compare_files objects "$tmpdir/${name}.git.objects" "$tmpdir/${name}.zmin.objects"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_gap() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  "$GIT_BIN" -C "$git_work" fsck --no-progress >"$tmpdir/${name}.git.fsck" 2>&1
  "$GIT_BIN" -C "$zmin_work" fsck --no-progress >"$tmpdir/${name}.zmin.fsck" 2>&1
  compare_files fsck "$tmpdir/${name}.git.fsck" "$tmpdir/${name}.zmin.fsck"
  object_files "$git_work" >"$tmpdir/${name}.git.objects"
  object_files "$zmin_work" >"$tmpdir/${name}.zmin.objects"
  if test "$git_exit" = "$zmin_exit" \
    && cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" \
    && cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err" \
    && cmp -s "$tmpdir/${name}.git.objects" "$tmpdir/${name}.zmin.objects"; then
    echo "$name unexpectedly matched" >&2
    exit 1
  fi
  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_exact gc_auto_long gc --auto
run_gap gc_quiet_short gc -q
run_gap gc_no_prune_long gc --no-prune --quiet
