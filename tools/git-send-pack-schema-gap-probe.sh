#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-send-pack-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_source() {
  local source="$1"
  "$GIT_BIN" init -q -b main "$source"
  "$GIT_BIN" -C "$source" config user.name "Oracle"
  "$GIT_BIN" -C "$source" config user.email "oracle@example.com"
  printf 'base\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add file.txt
  "$GIT_BIN" -C "$source" commit -qm "base"
  "$GIT_BIN" -C "$source" branch feature
  printf 'main\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add file.txt
  "$GIT_BIN" -C "$source" commit -qm "main"
}

make_remote_pair() {
  local root="$1"
  local seed="$2"
  mkdir -p "$root"
  "$GIT_BIN" -C "$root" init -q --bare git-remote.git
  "$GIT_BIN" -C "$root" init -q --bare zmin-remote.git
  if [ "$seed" = "main" ]; then
    "$GIT_BIN" -C "$source" push -q "$root/git-remote.git" refs/heads/feature:refs/heads/main
    "$GIT_BIN" -C "$source" push -q "$root/zmin-remote.git" refs/heads/feature:refs/heads/main
  fi
}

list_refs() {
  local git_dir="$1"
  "$GIT_BIN" --git-dir="$git_dir" for-each-ref --format='%(refname) %(objectname)' refs | sort
}

normalize_stderr() {
  local root="$1"
  local input="$2"
  local output="$3"
  sed \
    -e "s#${root}/git-remote.git#<remote>#g" \
    -e "s#${root}/zmin-remote.git#<remote>#g" \
    "$input" >"$output"
}

run_exact() {
  local name="$1"
  local seed="$2"
  local expected_git_exit="$3"
  local expected_zmin_exit="$4"
  local stdin_data="$5"
  shift 5
  local root="$tmpdir/$name"
  local git_work="$root/git-work"
  local zmin_work="$root/zmin-work"
  local git_exit=0
  local zmin_exit=0

  make_remote_pair "$root" "$seed"
  "$GIT_BIN" clone -q "$source" "$git_work"
  "$GIT_BIN" clone -q "$source" "$zmin_work"
  "$GIT_BIN" -C "$git_work" branch feature origin/feature >/dev/null
  "$GIT_BIN" -C "$zmin_work" branch feature origin/feature >/dev/null

  set +e
  printf '%s' "$stdin_data" | "$GIT_BIN" -C "$git_work" send-pack "$root/git-remote.git" "$@" >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  printf '%s' "$stdin_data" | "$ZMIN_BIN" -C "$zmin_work" send-pack "$root/zmin-remote.git" "$@" >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  list_refs "$root/git-remote.git" >"$root/git.refs"
  list_refs "$root/zmin-remote.git" >"$root/zmin.refs"

  test "$git_exit" = "$expected_git_exit"
  test "$zmin_exit" = "$expected_zmin_exit"
  normalize_stderr "$root" "$root/git.err" "$root/git.err.normalized"
  normalize_stderr "$root" "$root/zmin.err" "$root/zmin.err.normalized"
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err.normalized" "$root/zmin.err.normalized"
  cmp -s "$root/git.refs" "$root/zmin.refs"
  printf '%s\texact\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

source="$tmpdir/source"
make_source "$source"

run_exact send_pack_all empty 0 0 "" --all
run_exact send_pack_dry_run empty 0 0 "" --dry-run refs/heads/main
run_exact send_pack_force main 0 0 "" --force refs/heads/main
run_exact send_pack_receive_pack empty 0 0 "" --receive-pack=git-receive-pack refs/heads/main
run_exact send_pack_stdin empty 0 0 "refs/heads/main"$'\n' --stdin
run_exact send_pack_verbose empty 0 0 "" --verbose refs/heads/main
run_exact send_pack_force_short main 0 0 "" -f refs/heads/main
run_exact send_pack_dry_run_short empty 0 0 "" -n refs/heads/main
run_exact send_pack_verbose_short empty 0 0 "" -v refs/heads/main
