#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-apply-checkout-oracle.XXXXXX")"
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

make_apply_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'old\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  printf 'new\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" diff >"$repo/change.patch"
  "$GIT_BIN" -C "$repo" checkout -q -- a.txt
}

make_checkout_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'base\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  printf 'dirty\n' >"$repo/a.txt"
}

snapshot_worktree() {
  local repo="$1"
  local out_prefix="$2"
  "$GIT_BIN" -C "$repo" status --porcelain=v1 >"${out_prefix}.status"
  cat "$repo/a.txt" >"${out_prefix}.a_content"
}

compare_worktrees() {
  local name="$1"
  local git_repo="$2"
  local zmin_repo="$3"
  snapshot_worktree "$git_repo" "$tmpdir/${name}.git"
  snapshot_worktree "$zmin_repo" "$tmpdir/${name}.zmin"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  compare_files file "$tmpdir/${name}.git.a_content" "$tmpdir/${name}.zmin.a_content"
}

run_exact_apply_case() {
  local name="$1"
  local prep="$2"
  shift 2
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_apply_repo "$git_repo"
  make_apply_repo "$zmin_repo"
  if test "$prep" = "applied"; then
    "$GIT_BIN" -C "$git_repo" apply change.patch
    "$ZMIN_BIN" -C "$zmin_repo" apply change.patch
  fi

  set +e
  (cd "$git_repo" && "$GIT_BIN" apply "$@") >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (cd "$zmin_repo" && "$ZMIN_BIN" apply "$@") >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  compare_worktrees "$name" "$git_repo" "$zmin_repo"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_checkout_case() {
  local name="$1"
  local expected="$2"
  shift 2
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_checkout_repo "$git_repo"
  make_checkout_repo "$zmin_repo"

  set +e
  "$GIT_BIN" -C "$git_repo" checkout "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" checkout "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_worktrees "$name" "$git_repo" "$zmin_repo"
  if test "$expected" = "exact"; then
    compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
    compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
    printf '%s\tok\texit=%s\n' "$name" "$git_exit"
  elif cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" \
    && cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"; then
    echo "$name unexpectedly matched" >&2
    exit 1
  else
    printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  fi
}

run_exact_apply_case apply_patch_path clean change.patch
run_exact_apply_case apply_reverse_long applied --reverse change.patch
run_checkout_case checkout_force_long exact --force HEAD
run_checkout_case checkout_quiet_short gap -q .
