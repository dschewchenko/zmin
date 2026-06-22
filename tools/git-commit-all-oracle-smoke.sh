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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-commit-all-oracle.XXXXXX")"
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
  "$GIT_BIN" -C "$repo" config commit.gpgsign false
  printf 'base\n' >"$repo/tracked.txt"
  printf 'keep\n' >"$repo/other.txt"
  "$GIT_BIN" -C "$repo" add tracked.txt other.txt
  "$GIT_BIN" -C "$repo" commit -qm base
  printf 'changed\n' >"$repo/tracked.txt"
  printf 'new\n' >"$repo/untracked.txt"
}

run_case() {
  local name="$1"
  shift
  local seed_work="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_head="$tmpdir/${name}.git.head"
  local zmin_head="$tmpdir/${name}.zmin.head"
  local git_tree="$tmpdir/${name}.git.tree"
  local zmin_tree="$tmpdir/${name}.zmin.tree"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$seed_work"
  cp -R "$seed_work" "$git_work"
  cp -R "$seed_work" "$zmin_work"

  set +e
  (cd "$git_work" && "$GIT_BIN" -c commit.gpgsign=false commit "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" -c commit.gpgsign=false commit "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files status "$git_status" "$zmin_status"
  "$GIT_BIN" -C "$git_work" rev-parse HEAD >"$git_head"
  "$GIT_BIN" -C "$zmin_work" rev-parse HEAD >"$zmin_head"
  compare_files head "$git_head" "$zmin_head"
  "$GIT_BIN" -C "$git_work" rev-parse 'HEAD^{tree}' >"$git_tree"
  "$GIT_BIN" -C "$zmin_work" rev-parse 'HEAD^{tree}' >"$zmin_tree"
  compare_files tree "$git_tree" "$zmin_tree"
  "$GIT_BIN" -C "$git_work" cat-file -p HEAD >"$git_commit"
  "$GIT_BIN" -C "$zmin_work" cat-file -p HEAD >"$zmin_commit"
  compare_files commit "$git_commit" "$zmin_commit"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case commit_all_long --all -m all-long
run_case commit_all_short -a -m all-short
