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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-unpack-objects-oracle.XXXXXX")"
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

make_pack() {
  local repo="$tmpdir/source"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m one
  printf 'two\n' >>"$repo/a.txt"
  "$GIT_BIN" -C "$repo" commit -q -am two
  "$GIT_BIN" -C "$repo" pack-objects --stdout --revs >"$tmpdir/input.pack" <<'EOF'
HEAD
EOF
}

object_files() {
  local repo="$1"
  find "$repo/.git/objects" -type f | sed "s#^$repo/.git/objects/##" | sort
}

run_case() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  "$GIT_BIN" init -q "$git_repo"
  "$GIT_BIN" init -q "$zmin_repo"

  set +e
  "$GIT_BIN" -C "$git_repo" unpack-objects "$@" <"$tmpdir/input.pack" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" unpack-objects "$@" <"$tmpdir/input.pack" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  object_files "$git_repo" >"$tmpdir/${name}.git.objects"
  object_files "$zmin_repo" >"$tmpdir/${name}.zmin.objects"
  compare_files objects "$tmpdir/${name}.git.objects" "$tmpdir/${name}.zmin.objects"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

make_pack
run_case unpack_objects_dry_run_short -n
