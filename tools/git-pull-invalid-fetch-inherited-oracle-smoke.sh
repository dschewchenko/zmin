#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/debug/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-pull-invalid-fetch-inherited.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

export GIT_AUTHOR_NAME=Bench
export GIT_AUTHOR_EMAIL=bench@example.test
export GIT_AUTHOR_DATE="1700000000 +0000"
export GIT_COMMITTER_NAME=Bench
export GIT_COMMITTER_EMAIL=bench@example.test
export GIT_COMMITTER_DATE="1700000000 +0000"

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

seed_case_repo() {
  local root="$1"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"

  "$GIT_BIN" init -q -b main "$source"
  "$GIT_BIN" -C "$source" config user.name Bench
  "$GIT_BIN" -C "$source" config user.email bench@example.test
  printf 'base\n' >"$source/base.txt"
  "$GIT_BIN" -C "$source" add base.txt
  "$GIT_BIN" -C "$source" commit -q -m base

  "$GIT_BIN" clone -q "$source" "$git_client"
  "$GIT_BIN" clone -q "$source" "$zmin_client"
  "$GIT_BIN" -C "$git_client" config user.name Bench
  "$GIT_BIN" -C "$git_client" config user.email bench@example.test
  "$GIT_BIN" -C "$zmin_client" config user.name Bench
  "$GIT_BIN" -C "$zmin_client" config user.email bench@example.test

  printf 'next\n' >"$source/next.txt"
  "$GIT_BIN" -C "$source" add next.txt
  "$GIT_BIN" -C "$source" commit -q -m next
}

compare_optional_fetch_head() {
  local git_client="$1"
  local zmin_client="$2"
  local git_fetch="$git_client/.git/FETCH_HEAD"
  local zmin_fetch="$zmin_client/.git/FETCH_HEAD"
  if test -f "$git_fetch" || test -f "$zmin_fetch"; then
    test -f "$git_fetch"
    test -f "$zmin_fetch"
    compare_files fetch_head "$git_fetch" "$zmin_fetch"
  fi
}

run_case() {
  local name="$1"
  shift
  local root="$tmpdir/$name"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  mkdir -p "$root"
  seed_case_repo "$root"

  "$GIT_BIN" -C "$git_client" rev-parse HEAD >"$root/git.head.before"
  "$GIT_BIN" -C "$zmin_client" rev-parse HEAD >"$root/zmin.head.before"

  set +e
  "$GIT_BIN" -C "$git_client" pull --ff-only "$@" origin main >"$root/git.out" 2>"$root/git.err"
  local git_rc=$?
  "$ZMIN_BIN" -C "$zmin_client" pull --ff-only "$@" origin main >"$root/zmin.out" 2>"$root/zmin.err"
  local zmin_rc=$?
  set -e

  test "$git_rc" -eq "$zmin_rc"
  compare_files stdout "$root/git.out" "$root/zmin.out"
  compare_files stderr "$root/git.err" "$root/zmin.err"

  "$GIT_BIN" -C "$git_client" rev-parse HEAD >"$root/git.head.after"
  "$GIT_BIN" -C "$zmin_client" rev-parse HEAD >"$root/zmin.head.after"
  compare_files git_head_stable "$root/git.head.before" "$root/git.head.after"
  compare_files zmin_head_stable "$root/zmin.head.before" "$root/zmin.head.after"
  compare_files heads_match "$root/git.head.after" "$root/zmin.head.after"

  "$GIT_BIN" -C "$git_client" status --porcelain=v1 --branch >"$root/git.status"
  "$GIT_BIN" -C "$zmin_client" status --porcelain=v1 --branch >"$root/zmin.status"
  compare_files status "$root/git.status" "$root/zmin.status"
  compare_optional_fetch_head "$git_client" "$zmin_client"
}

run_case pull_atomic_unknown --atomic
run_case pull_auto_gc_unknown --auto-gc
run_case pull_auto_maintenance_unknown --auto-maintenance
run_case pull_multiple_unknown --multiple
run_case pull_negotiate_only_unknown --negotiate-only
run_case pull_no_auto_gc_unknown --no-auto-gc
run_case pull_no_auto_maintenance_unknown --no-auto-maintenance
run_case pull_no_write_commit_graph_unknown --no-write-commit-graph
run_case pull_no_write_fetch_head_unknown --no-write-fetch-head
run_case pull_porcelain_unknown --porcelain
run_case pull_prefetch_unknown --prefetch
run_case pull_prune_tags_unknown --prune-tags
run_case pull_recurse_submodules_default_unknown --recurse-submodules-default=yes
run_case pull_refetch_unknown --refetch
run_case pull_submodule_prefix_unknown --submodule-prefix=foo/
run_case pull_update_head_ok_unknown --update-head-ok
run_case pull_write_commit_graph_unknown --write-commit-graph
run_case pull_write_fetch_head_unknown --write-fetch-head
run_case pull_short_prefetch_unknown -P
run_case pull_short_edit_unknown -e
run_case pull_short_set_upstream_unknown -u
