#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-filter-branch-extra.XXXXXX")"
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

make_source_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.test"
  printf 'hello\n' >"$repo/README.md"
  "$GIT_BIN" -C "$repo" add -A
  GIT_AUTHOR_DATE="2030-01-01T00:00:00 +0000" \
    GIT_COMMITTER_DATE="2030-01-01T00:00:00 +0000" \
    "$GIT_BIN" -C "$repo" commit -qm "base"
}

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  "$GIT_BIN" clone -q "$source_repo" "$git_work"
  "$GIT_BIN" clone -q "$source_repo" "$zmin_work"

  set +e
  (
    cd "$git_work"
    FILTER_BRANCH_SQUELCH_WARNING=1 \
      GIT_AUTHOR_DATE="2030-01-02T00:00:00 +0000" \
      GIT_COMMITTER_DATE="2030-01-02T00:00:00 +0000" \
      "$GIT_BIN" -c commit.gpgsign=false filter-branch "$@"
  ) >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (
    cd "$zmin_work"
    FILTER_BRANCH_SQUELCH_WARNING=1 \
      GIT_AUTHOR_DATE="2030-01-02T00:00:00 +0000" \
      GIT_COMMITTER_DATE="2030-01-02T00:00:00 +0000" \
      "$ZMIN_BIN" filter-branch "$@"
  ) >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  "$GIT_BIN" -C "$git_work" rev-parse HEAD >"$tmpdir/${name}.git.head"
  "$GIT_BIN" -C "$zmin_work" rev-parse HEAD >"$tmpdir/${name}.zmin.head"
  compare_files head "$tmpdir/${name}.git.head" "$tmpdir/${name}.zmin.head"
  "$GIT_BIN" -C "$git_work" rev-parse 'HEAD^{tree}' >"$tmpdir/${name}.git.tree"
  "$GIT_BIN" -C "$zmin_work" rev-parse 'HEAD^{tree}' >"$tmpdir/${name}.zmin.tree"
  compare_files tree "$tmpdir/${name}.git.tree" "$tmpdir/${name}.zmin.tree"
  "$GIT_BIN" -C "$git_work" rev-parse refs/backup/refs/heads/main >"$tmpdir/${name}.git.backup"
  "$GIT_BIN" -C "$zmin_work" rev-parse refs/backup/refs/heads/main >"$tmpdir/${name}.zmin.backup"
  compare_files backup "$tmpdir/${name}.git.backup" "$tmpdir/${name}.zmin.backup"
  "$GIT_BIN" -C "$git_work" log --format='%an <%ae>|%s|%b' --reverse >"$tmpdir/${name}.git.log"
  "$GIT_BIN" -C "$zmin_work" log --format='%an <%ae>|%s|%b' --reverse >"$tmpdir/${name}.zmin.log"
  compare_files log "$tmpdir/${name}.git.log" "$tmpdir/${name}.zmin.log"
  "$GIT_BIN" -C "$git_work" status --porcelain=v1 -uno >"$tmpdir/${name}.git.status"
  "$GIT_BIN" -C "$zmin_work" status --porcelain=v1 -uno >"$tmpdir/${name}.zmin.status"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

source_repo="$tmpdir/source"
make_source_repo "$source_repo"

run_case filter_branch_force_original_head --force --original refs/backup --msg-filter cat HEAD
