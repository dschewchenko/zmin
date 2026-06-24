#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-mv-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

seed_repo() {
  local repo="$1"
  local tracked_destination="${2:-false}"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'a\n' >"$repo/a.txt"
  if [ "$tracked_destination" = true ]; then
    printf 'b\n' >"$repo/b.txt"
  fi
  mkdir "$repo/dst"
  "$GIT_BIN" -C "$repo" add a.txt b.txt 2>/dev/null || "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -qm base
}

record_state() {
  local repo="$1"
  local prefix="$2"
  "$GIT_BIN" -C "$repo" status --short >"$prefix.status"
  "$GIT_BIN" -C "$repo" ls-files -s >"$prefix.index"
  find "$repo" -maxdepth 1 -type f -print | sort | sed "s#$repo/##" >"$prefix.files"
}

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

run_exact() {
  local name="$1"
  shift
  local git_work="$tmpdir/$name.git"
  local zmin_work="$tmpdir/$name.zmin"
  local git_exit=0
  local zmin_exit=0
  local tracked_destination=false

  if [ "${1:-}" = "--tracked-destination" ]; then
    tracked_destination=true
    shift
  fi

  seed_repo "$git_work" "$tracked_destination"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" mv "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" mv "$@" >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out"
  compare_files stderr "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"
  record_state "$git_work" "$tmpdir/$name.git"
  record_state "$zmin_work" "$tmpdir/$name.zmin"
  compare_files status "$tmpdir/$name.git.status" "$tmpdir/$name.zmin.status"
  compare_files index "$tmpdir/$name.git.index" "$tmpdir/$name.zmin.index"
  compare_files files "$tmpdir/$name.git.files" "$tmpdir/$name.zmin.files"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_exact mv_dry_run_short -n a.txt b.txt
run_exact mv_dry_run_long --dry-run a.txt b.txt
run_exact mv_verbose_short -v a.txt b.txt
run_exact mv_verbose_long --verbose a.txt b.txt
run_exact mv_skip_errors_short -k missing.txt a.txt dst
run_exact mv_dry_run_verbose_short -n -v a.txt b.txt
run_exact mv_force_dry_run_short --tracked-destination -f -n a.txt b.txt
run_exact mv_dry_run_verbose_long --dry-run --verbose a.txt b.txt
run_exact mv_force_dry_run_long --tracked-destination --force --dry-run a.txt b.txt
run_exact mv_skip_errors_repeated -k -k missing.txt a.txt dst
run_exact mv_skip_errors_dry_run -k -n missing.txt a.txt dst
run_exact mv_force_verbose_short --tracked-destination -f -v a.txt b.txt
run_exact mv_force_verbose_long --tracked-destination --force --verbose a.txt b.txt
run_exact mv_force_verbose_dry_run --tracked-destination -f -v -n a.txt b.txt
