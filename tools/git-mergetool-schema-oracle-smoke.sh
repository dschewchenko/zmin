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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-mergetool-oracle.XXXXXX")"
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

seed_conflict_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'base\n' >"$repo/f.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" checkout -q -b feature
  printf 'feature\n' >"$repo/f.txt"
  "$GIT_BIN" -C "$repo" commit -q -am feature
  "$GIT_BIN" -C "$repo" checkout -q main
  printf 'main\n' >"$repo/f.txt"
  "$GIT_BIN" -C "$repo" commit -q -am main
  set +e
  "$GIT_BIN" -C "$repo" merge feature >/dev/null 2>/dev/null
  set -e
  "$GIT_BIN" -C "$repo" config mergetool.zmintest.cmd \
    "printf 'B:'; cat \"\$BASE\"; printf 'L:'; cat \"\$LOCAL\"; printf 'R:'; cat \"\$REMOTE\"; printf 'resolved\\n' > \"\$MERGED\""
  "$GIT_BIN" -C "$repo" config mergetool.zmintest.trustExitCode true
}

record_state() {
  local repo="$1"
  local prefix="$2"
  "$GIT_BIN" -C "$repo" status --short >"$prefix.status"
  "$GIT_BIN" -C "$repo" ls-files -s >"$prefix.index"
  cat "$repo/f.txt" >"$prefix.file"
}

run_exact() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  seed_conflict_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" mergetool "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" mergetool "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  record_state "$git_work" "$tmpdir/${name}.git"
  record_state "$zmin_work" "$tmpdir/${name}.zmin"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  compare_files index "$tmpdir/${name}.git.index" "$tmpdir/${name}.zmin.index"
  compare_files file "$tmpdir/${name}.git.file" "$tmpdir/${name}.zmin.file"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_exact mergetool_tool_short -t zmintest --no-prompt f.txt
run_exact mergetool_no_prompt_short --tool=zmintest -y f.txt
