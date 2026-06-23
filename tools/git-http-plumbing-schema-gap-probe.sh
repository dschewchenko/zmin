#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-http-plumbing-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

run_case() {
  local name="$1"
  shift
  local command="$1"
  shift
  local git_exit=0
  local zmin_exit=0

  set +e
  (
    cd "$tmpdir"
    "$GIT_BIN" "$command" "$@"
  ) >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (
    cd "$tmpdir"
    "$ZMIN_BIN" "$command" "$@"
  ) >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  case "$name" in
    http_fetch_positional_outside_repo)
      test "$git_exit" = 128
      test "$zmin_exit" = 128
      cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
      cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
      printf '%s\tok\texit=%s\n' "$name" "$git_exit"
      ;;
    http_fetch_*_usage_gap)
      test "$git_exit" = 129
      test "$zmin_exit" = 129
      cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
      cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
      printf '%s\tok\texit=%s\n' "$name" "$git_exit"
      ;;
    http_push_*_outside_repo_gap)
      test "$git_exit" = 128
      test "$zmin_exit" = 128
      cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
      cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
      printf '%s\tok\texit=%s\n' "$name" "$git_exit"
      ;;
    *)
      echo "unknown probe case: $name" >&2
      return 1
      ;;
  esac
}

run_case http_fetch_recover_usage_gap http-fetch --recover
run_case http_fetch_all_usage_gap http-fetch -a
run_case http_fetch_commit_usage_gap http-fetch -c
run_case http_fetch_tags_usage_gap http-fetch -t
run_case http_fetch_verbose_usage_gap http-fetch -v
run_case http_fetch_write_ref_usage_gap http-fetch -w refs/heads/main
run_case http_fetch_positional_outside_repo http-fetch deadbeef http://127.0.0.1:1/repo.git

run_case http_push_all_outside_repo_gap http-push --all http://127.0.0.1:1/repo.git
run_case http_push_dry_run_outside_repo_gap http-push --dry-run http://127.0.0.1:1/repo.git
run_case http_push_force_outside_repo_gap http-push --force http://127.0.0.1:1/repo.git
run_case http_push_verbose_outside_repo_gap http-push --verbose http://127.0.0.1:1/repo.git
run_case http_push_remote_outside_repo_gap http-push http://127.0.0.1:1/repo.git
run_case http_push_heads_outside_repo_gap http-push http://127.0.0.1:1/repo.git refs/heads/main
