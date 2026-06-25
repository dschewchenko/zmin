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

tmpdir="$(mktemp -d /tmp/zmin-show-ref-schema-oracle.XXXXXX)"
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
  "$GIT_BIN" -C "$repo" tag -a v1 -m v1
}

run_case() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_refs="$tmpdir/${name}.git.refs"
  local zmin_refs="$tmpdir/${name}.zmin.refs"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$seed"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" show-ref >"$git_refs"
  "$GIT_BIN" -C "$zmin_work" show-ref >"$zmin_refs"
  compare_files refs "$git_refs" "$zmin_refs"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files worktree_status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case show_ref_positional_refs show-ref refs/heads/main
run_case show_ref_hash_short show-ref -s
run_case show_ref_hash_long show-ref --hash
run_case show_ref_hash_long_value show-ref --hash=12
run_case show_ref_head_long show-ref --head
run_case show_ref_branches_alias show-ref --branches
run_case show_ref_dereference_long show-ref --dereference
run_case show_ref_dereference_short show-ref -d
run_case show_ref_head_hash show-ref --head --hash
run_case show_ref_hash_head show-ref --hash --head
run_case show_ref_head_heads_hash show-ref --head --heads --hash
run_case show_ref_hash_head_heads show-ref --hash --head --heads
run_case show_ref_head_heads_hash_value show-ref --head --heads --hash=12
run_case show_ref_hash_value_head_heads show-ref --hash=12 --head --heads
run_case show_ref_heads_hash show-ref --heads --hash
run_case show_ref_heads_hash_value show-ref --heads --hash=12
run_case show_ref_hash_heads show-ref --hash --heads
run_case show_ref_tags_hash show-ref --tags --hash
run_case show_ref_tags_hash_value show-ref --tags --hash=12
run_case show_ref_hash_tags show-ref --hash --tags
run_case show_ref_head_tags_hash show-ref --head --tags --hash
run_case show_ref_hash_head_tags show-ref --hash --head --tags
run_case show_ref_head_tags_hash_value show-ref --head --tags --hash=12
run_case show_ref_hash_value_head_tags show-ref --hash=12 --head --tags
run_case show_ref_heads_tags_hash show-ref --heads --tags --hash
run_case show_ref_hash_heads_tags show-ref --hash --heads --tags
run_case show_ref_heads_tags_hash_value show-ref --heads --tags --hash=12
run_case show_ref_head_heads_tags_hash show-ref --head --heads --tags --hash
run_case show_ref_hash_head_heads_tags show-ref --hash --head --heads --tags
run_case show_ref_head_heads_tags_hash_value show-ref --head --heads --tags --hash=12
run_case show_ref_head_branches_tags_hash show-ref --head --branches --tags --hash
run_case show_ref_hash_head_branches_tags show-ref --hash --head --branches --tags
run_case show_ref_head_branches_tags_hash_value show-ref --head --branches --tags --hash=12
run_case show_ref_hash_value_head_branches_tags show-ref --hash=12 --head --branches --tags
run_case show_ref_branches_hash show-ref --branches --hash
run_case show_ref_branches_hash_value show-ref --branches --hash=12
run_case show_ref_hash_branches show-ref --hash --branches
run_case show_ref_head_branches_hash show-ref --head --branches --hash
run_case show_ref_hash_head_branches show-ref --hash --head --branches
run_case show_ref_deref_hash show-ref --dereference --hash
run_case show_ref_hash_deref show-ref --hash --dereference
run_case show_ref_deref_tags show-ref --dereference --tags
run_case show_ref_tags_deref show-ref --tags --dereference
run_case show_ref_verify_hash_existing show-ref --verify --hash refs/heads/main
run_case show_ref_verify_head_hash_existing show-ref --verify --head --hash refs/heads/main
run_case show_ref_verify_heads_hash_existing show-ref --verify --heads --hash refs/heads/main
run_case show_ref_verify_hash_value_existing show-ref --verify --hash=12 refs/heads/main
run_case show_ref_verify_head_hash_value_existing show-ref --verify --head --hash=12 refs/heads/main
run_case show_ref_verify_heads_hash_value_existing show-ref --verify --heads --hash=12 refs/heads/main
run_case show_ref_verify_branches_hash_value_existing show-ref --verify --branches --hash=12 refs/heads/main
run_case show_ref_verify_head_heads_hash_existing show-ref --verify --head --heads --hash refs/heads/main
run_case show_ref_verify_head_heads_hash_value_existing show-ref --verify --head --heads --hash=12 refs/heads/main
run_case show_ref_verify_head_tags_hash_value_tag show-ref --verify --head --tags --hash=12 refs/tags/v1
run_case show_ref_verify_tags_hash_value_tag show-ref --verify --tags --hash=12 refs/tags/v1
run_case show_ref_verify_hash_value_tag show-ref --verify --hash=12 refs/tags/v1
run_case show_ref_verify_hash_missing show-ref --verify --hash refs/heads/missing
