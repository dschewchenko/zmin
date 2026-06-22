#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-update-ref-oracle.XXXXXX")"
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
  printf 'base\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add file.txt
  "$GIT_BIN" -C "$repo" commit -qm "base"
  "$GIT_BIN" -C "$repo" branch old
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
  local git_refs="$tmpdir/${name}.git.refs"
  local zmin_refs="$tmpdir/${name}.zmin.refs"
  local git_reflog="$tmpdir/${name}.git.reflog"
  local zmin_reflog="$tmpdir/${name}.zmin.reflog"
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
  "$GIT_BIN" -C "$git_work" for-each-ref --format='%(refname)%00%(objectname)' >"$git_refs"
  "$GIT_BIN" -C "$zmin_work" for-each-ref --format='%(refname)%00%(objectname)' >"$zmin_refs"
  compare_files refs "$git_refs" "$zmin_refs"
  "$GIT_BIN" -C "$git_work" reflog show --all --format='%gD %H %gs' >"$git_reflog" 2>/dev/null || true
  "$GIT_BIN" -C "$zmin_work" reflog show --all --format='%gD %H %gs' >"$zmin_reflog" 2>/dev/null || true
  compare_files reflog "$git_reflog" "$zmin_reflog"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

base_seed="$tmpdir/base-for-oid"
make_seed_repo "$base_seed"
head_oid="$("$GIT_BIN" -C "$base_seed" rev-parse HEAD)"

run_case update_ref_positional "" update-ref refs/heads/new "$head_oid"
run_case update_ref_message "" update-ref -m msg refs/heads/new "$head_oid"
run_case update_ref_no_create_reflog "" update-ref --no-create-reflog refs/heads/new "$head_oid"
run_case update_ref_delete_short "" update-ref -d refs/heads/old
run_case update_ref_stdin "update refs/heads/new $head_oid\n" update-ref --stdin
run_case update_ref_stdin_batch "update refs/heads/new $head_oid\n" update-ref --stdin --batch-updates
run_case update_ref_stdin_no_batch "update refs/heads/new $head_oid\n" update-ref --stdin --no-batch-updates
