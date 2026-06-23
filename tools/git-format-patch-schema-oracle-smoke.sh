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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-format-patch-oracle.XXXXXX")"
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

normalize_patch_stream() {
  local input="$1"
  local output="$2"
  sed -E \
    -e 's/(boundary=")[^"]+"/\1BOUNDARY"/g' \
    -e 's/^--[-_A-Za-z0-9. ()]+(--)?$/--BOUNDARY\1/g' \
    -e 's/^(X-Mailer: git-format-patch version ).*/\1VERSION/' \
    -e 's/^(-- )[-_A-Za-z0-9. ()]+$/\1VERSION/' \
    -e 's/^([0-9]+\.[0-9][-_A-Za-z0-9. ()]*|0\.1\.0\.zmin)$/VERSION/' \
    "$input" >"$output"
}

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'base\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  printf 'one\n' >>"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m one
  printf 'two\n' >>"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m two
}

compare_status() {
  local name="$1"
  local git_work="$2"
  local zmin_work="$3"
  "$GIT_BIN" -C "$git_work" status --short >"$tmpdir/${name}.git.status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$tmpdir/${name}.zmin.status"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
}

compare_patch_dirs() {
  local name="$1"
  local git_dir="$2"
  local zmin_dir="$3"
  (cd "$git_dir" && find . -type f | sort) >"$tmpdir/${name}.git.files"
  (cd "$zmin_dir" && find . -type f | sort) >"$tmpdir/${name}.zmin.files"
  compare_files files "$tmpdir/${name}.git.files" "$tmpdir/${name}.zmin.files"
  while IFS= read -r path; do
    normalize_patch_stream "$git_dir/$path" "$tmpdir/${name}.git.patch"
    normalize_patch_stream "$zmin_dir/$path" "$tmpdir/${name}.zmin.patch"
    compare_files "patch $path" "$tmpdir/${name}.git.patch" "$tmpdir/${name}.zmin.patch"
  done <"$tmpdir/${name}.git.files"
}

run_stdout_case() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$seed"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" format-patch "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" format-patch "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  normalize_patch_stream "$tmpdir/${name}.git.out" "$tmpdir/${name}.git.norm"
  normalize_patch_stream "$tmpdir/${name}.zmin.out" "$tmpdir/${name}.zmin.norm"
  compare_files stdout "$tmpdir/${name}.git.norm" "$tmpdir/${name}.zmin.norm"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  compare_status "$name" "$git_work" "$zmin_work"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_directory_case() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$seed"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"
  mkdir "$git_work/out" "$zmin_work/out"

  set +e
  "$GIT_BIN" -C "$git_work" format-patch "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" format-patch "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  compare_patch_dirs "$name" "$git_work/out" "$zmin_work/out"
  compare_status "$name" "$git_work" "$zmin_work"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_directory_gap() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$seed"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"
  mkdir "$git_work/out" "$zmin_work/out"

  set +e
  "$GIT_BIN" -C "$git_work" format-patch "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" format-patch "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  (cd "$git_work/out" && find . -type f | sort) >"$tmpdir/${name}.git.files"
  (cd "$zmin_work/out" && find . -type f | sort) >"$tmpdir/${name}.zmin.files"

  if test "$git_exit" = "$zmin_exit" &&
    cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" &&
    cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err" &&
    cmp -s "$tmpdir/${name}.git.files" "$tmpdir/${name}.zmin.files"; then
    echo "$name unexpectedly matches stock Git; update the open matrix row" >&2
    return 1
  fi

  compare_status "$name" "$git_work" "$zmin_work"
  printf '%s\tgap\tgit_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_stdout_case format_patch_attach_long --stdout --attach HEAD~2..HEAD
run_stdout_case format_patch_numbered_long --stdout --numbered HEAD~2..HEAD
run_directory_case format_patch_output_directory_long --output-directory out HEAD~2..HEAD
run_directory_case format_patch_numbered_files_long --output-directory out --numbered-files HEAD~2..HEAD
run_directory_case format_patch_suffix_long --output-directory out --suffix=.mbox HEAD~2..HEAD
