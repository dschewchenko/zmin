#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-fetch-pack-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_remote() {
  local root="$1"
  mkdir "$root"
  "$GIT_BIN" -C "$root" init -q --bare remote.git
  "$GIT_BIN" -C "$root" init -q -b main work
  "$GIT_BIN" -C "$root/work" config user.name "Oracle"
  "$GIT_BIN" -C "$root/work" config user.email "oracle@example.com"
  printf 'base\n' >"$root/work/file.txt"
  "$GIT_BIN" -C "$root/work" add file.txt
  "$GIT_BIN" -C "$root/work" commit -qm "base"
  "$GIT_BIN" -C "$root/work" remote add origin "$root/remote.git"
  "$GIT_BIN" -C "$root/work" push -q origin main
  "$GIT_BIN" -C "$root/remote.git" symbolic-ref HEAD refs/heads/main
}

make_client() {
  local path="$1"
  "$GIT_BIN" init -q "$path"
}

run_gap() {
  local name="$1"
  local expected_git_exit="$2"
  local expected_zmin_exit="$3"
  local stdin_data="$4"
  shift 4
  local git_client="$tmpdir/$name.git-client"
  local zmin_client="$tmpdir/$name.zmin-client"
  local git_exit=0
  local zmin_exit=0

  make_client "$git_client"
  make_client "$zmin_client"

  set +e
  printf '%s' "$stdin_data" | "$GIT_BIN" -C "$git_client" fetch-pack "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  printf '%s' "$stdin_data" | "$ZMIN_BIN" -C "$zmin_client" fetch-pack "$@" >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  printf 'stock stdout:\n'
  sed -n '1,6p' "$tmpdir/$name.git.out"
  printf 'zmin stdout:\n'
  sed -n '1,6p' "$tmpdir/$name.zmin.out"
  printf 'stock stderr:\n'
  sed -n '1,4p' "$tmpdir/$name.git.err"
  printf 'zmin stderr:\n'
  sed -n '1,4p' "$tmpdir/$name.zmin.err"

  test "$git_exit" = "$expected_git_exit"
  test "$zmin_exit" = "$expected_zmin_exit"
  if [ "$git_exit" = "$zmin_exit" ] \
    && cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out" \
    && cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"; then
    echo "$name unexpectedly matched" >&2
    return 1
  fi
}

root="$tmpdir/root"
make_remote "$root"
remote="$root/remote.git"

run_gap fetch_pack_all 0 0 "" --all "$remote"
run_gap fetch_pack_stdin 0 0 "refs/heads/main"$'\n' --stdin "$remote"
run_gap fetch_pack_quiet 0 0 "" --quiet "$remote" refs/heads/main
run_gap fetch_pack_keep 0 129 "" --keep "$remote" refs/heads/main
run_gap fetch_pack_upload_pack 0 129 "" --upload-pack=git-upload-pack "$remote" refs/heads/main
run_gap fetch_pack_diag_url 0 129 "" --diag-url "$remote"
run_gap fetch_pack_verbose_long 129 129 "" --verbose "$remote" refs/heads/main
run_gap fetch_pack_keep_short 0 129 "" -k "$remote" refs/heads/main
run_gap fetch_pack_quiet_short 0 0 "" -q "$remote" refs/heads/main
run_gap fetch_pack_verbose_short 0 129 "" -v "$remote" refs/heads/main
