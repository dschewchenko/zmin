#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-difftool-short-alias.XXXXXX")"
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
  printf 'old-a\n' >"$repo/a.txt"
  printf 'old-b\n' >"$repo/b.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -qm "base"
}

run_case() {
  local name="$1"
  local stdin_text="$2"
  shift 2
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_exit=0
  local zmin_exit=0

  cp -R "$base_seed" "$git_work"
  cp -R "$base_seed" "$zmin_work"
  printf 'new-a\n' >"$git_work/a.txt"
  printf 'new-b\n' >"$git_work/b.txt"
  printf 'new-a\n' >"$zmin_work/a.txt"
  printf 'new-b\n' >"$zmin_work/b.txt"
  "$GIT_BIN" -C "$git_work" config diff.tool zmintest
  "$GIT_BIN" -C "$git_work" config difftool.zmintest.cmd "printf 'L:'; cat \"\$LOCAL\"; printf 'R:'; cat \"\$REMOTE\""
  "$GIT_BIN" -C "$git_work" config difftool.prompt false
  "$GIT_BIN" -C "$zmin_work" config diff.tool zmintest
  "$GIT_BIN" -C "$zmin_work" config difftool.zmintest.cmd "printf 'L:'; cat \"\$LOCAL\"; printf 'R:'; cat \"\$REMOTE\""
  "$GIT_BIN" -C "$zmin_work" config difftool.prompt false

  set +e
  (cd "$git_work" && printf '%b' "$stdin_text" | "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && printf '%b' "$stdin_text" | "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --porcelain=v1 -uno >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --porcelain=v1 -uno >"$zmin_status"
  compare_files status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

base_seed="$tmpdir/base"
make_seed_repo "$base_seed"
extcmd="sh -c 'printf \"L:\"; cat \"\$1\"; printf \"R:\"; cat \"\$2\"' _"

run_case difftool_extcmd_short_alias "" difftool -y -x "$extcmd" a.txt
run_case difftool_tool_short_alias "" difftool -y -t zmintest a.txt
run_case difftool_no_prompt_short_alias "" difftool -y a.txt
run_case difftool_positional_path "" difftool -y -t zmintest a.txt
run_case difftool_prompt_long "y\n" difftool --prompt -t zmintest a.txt
