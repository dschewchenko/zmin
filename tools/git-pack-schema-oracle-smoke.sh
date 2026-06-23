#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-pack-schema-oracle.XXXXXX")"
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
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'content\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -qm initial
}

make_pack_file() {
  local repo="$1"
  "$GIT_BIN" -C "$repo" pack-objects --all "$repo/input" >/dev/null
  find "$repo" -maxdepth 1 -name 'input-*.pack' | head -1
}

run_index_pack_exact() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_pack
  local zmin_pack
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"
  git_pack="$(make_pack_file "$git_work")"
  zmin_pack="$(make_pack_file "$zmin_work")"

  set +e
  (cd "$git_work" && "$GIT_BIN" index-pack "$@" "$git_pack") >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" index-pack "$@" "$zmin_pack") >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  compare_files out_idx "$git_work/out.idx" "$zmin_work/out.idx"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_index_pack_gap() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_pack
  local zmin_pack
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"
  git_pack="$(make_pack_file "$git_work")"
  zmin_pack="$(make_pack_file "$zmin_work")"

  set +e
  (cd "$git_work" && "$GIT_BIN" index-pack "$@" "$git_pack") >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" index-pack "$@" "$zmin_pack") >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if test "$git_exit" = "$zmin_exit" \
    && cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" \
    && cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"; then
    echo "$name unexpectedly matched" >&2
    exit 1
  fi
  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_repack_exact() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" repack "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" repack "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  "$GIT_BIN" -C "$git_work" fsck --no-progress >"$tmpdir/${name}.git.fsck" 2>&1
  "$GIT_BIN" -C "$zmin_work" fsck --no-progress >"$tmpdir/${name}.zmin.fsck" 2>&1
  compare_files fsck "$tmpdir/${name}.git.fsck" "$tmpdir/${name}.zmin.fsck"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_repack_gap() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" repack "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" repack "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if test "$git_exit" = "$zmin_exit" \
    && cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" \
    && cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"; then
    echo "$name unexpectedly matched" >&2
    exit 1
  fi
  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_index_pack_exact index_pack_output_short -o out.idx
run_index_pack_gap index_pack_verbose_short -v
run_repack_exact repack_quiet_long --quiet
run_repack_gap repack_threads_long --threads=1
