#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d /tmp/zmin-write-tree-schema-oracle.XXXXXX)"
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

seed_repo_with_missing_index_blob() {
  local bin="$1"
  local repo="$2"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  printf 'one\n' >"$repo/a.txt"
  "$bin" -C "$repo" add a.txt
  local blob
  blob="$("$GIT_BIN" -C "$repo" hash-object a.txt)"
  rm -f "$repo/.git/objects/${blob:0:2}/${blob:2}"
}

run_case() {
  local name="$1"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_tree_type="$tmpdir/${name}.git.tree-type"
  local zmin_tree_type="$tmpdir/${name}.zmin.tree-type"
  local git_exit=0
  local zmin_exit=0

  seed_repo_with_missing_index_blob "$GIT_BIN" "$git_work"
  seed_repo_with_missing_index_blob "$ZMIN_BIN" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" write-tree --missing-ok >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" write-tree --missing-ok >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" cat-file -t "$(cat "$git_out")" >"$git_tree_type"
  "$GIT_BIN" -C "$zmin_work" cat-file -t "$(cat "$zmin_out")" >"$zmin_tree_type"
  compare_files tree_type "$git_tree_type" "$zmin_tree_type"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

seed_repo_with_prefix_tree() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  mkdir "$repo/src"
  printf 'root\n' >"$repo/root.txt"
  printf 'src\n' >"$repo/src/a.txt"
  "$GIT_BIN" -C "$repo" add root.txt src/a.txt
}

run_prefix_missing_case() {
  local name="$1"
  local prefix="$2"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0

  seed_repo_with_prefix_tree "$git_work"
  seed_repo_with_prefix_tree "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" write-tree "--prefix=$prefix" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" write-tree "--prefix=$prefix" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 128
  test "$zmin_exit" = 128
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_prefix_success_case() {
  local name="$1"
  local prefix="$2"
  shift 2
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0

  seed_repo_with_prefix_tree "$git_work"
  seed_repo_with_prefix_tree "$zmin_work"
  mkdir "$git_work/src/inner" "$zmin_work/src/inner"
  printf 'inner\n' >"$git_work/src/inner/b.txt"
  printf 'inner\n' >"$zmin_work/src/inner/b.txt"
  "$GIT_BIN" -C "$git_work" add src/inner/b.txt
  "$GIT_BIN" -C "$zmin_work" add src/inner/b.txt

  set +e
  "$GIT_BIN" -C "$git_work" write-tree "$@" "--prefix=$prefix" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" write-tree "$@" "--prefix=$prefix" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 0
  test "$zmin_exit" = 0
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_prefix_first_success_case() {
  local name="$1"
  local prefix="$2"
  shift 2
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0

  seed_repo_with_prefix_tree "$git_work"
  seed_repo_with_prefix_tree "$zmin_work"
  mkdir "$git_work/src/inner" "$zmin_work/src/inner"
  printf 'inner\n' >"$git_work/src/inner/b.txt"
  printf 'inner\n' >"$zmin_work/src/inner/b.txt"
  "$GIT_BIN" -C "$git_work" add src/inner/b.txt
  "$GIT_BIN" -C "$zmin_work" add src/inner/b.txt

  set +e
  "$GIT_BIN" -C "$git_work" write-tree "--prefix=$prefix" "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" write-tree "--prefix=$prefix" "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 0
  test "$zmin_exit" = 0
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_default_success_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0

  seed_repo_with_prefix_tree "$git_work"
  seed_repo_with_prefix_tree "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" write-tree "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" write-tree "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 0
  test "$zmin_exit" = 0
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_default_invalid_case() {
  local name="$1"
  local expected_exit="$2"
  shift 2
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0

  seed_repo_with_prefix_tree "$git_work"
  seed_repo_with_prefix_tree "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" write-tree "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" write-tree "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$expected_exit"
  test "$zmin_exit" = "$expected_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case write_tree_missing_ok
run_default_success_case write_tree_no_missing_ok --no-missing-ok
run_default_success_case write_tree_no_prefix --no-prefix
run_default_success_case write_tree_prefix_space_value --prefix src
run_prefix_success_case write_tree_prefix_trailing_slash src/
run_default_success_case write_tree_prefix_empty --prefix=
run_default_success_case write_tree_missing_ok_then_no_missing_ok --missing-ok --no-missing-ok
run_default_success_case write_tree_no_missing_ok_then_missing_ok --no-missing-ok --missing-ok
run_default_success_case write_tree_no_prefix_then_prefix --no-prefix --prefix=src
run_default_success_case write_tree_prefix_then_no_prefix --prefix=src --no-prefix
run_prefix_first_success_case write_tree_repeated_prefix_last_wins src --prefix=src/inner
run_prefix_success_case write_tree_prefix_double_slash src//inner
run_prefix_success_case write_tree_prefix_triple_slash src///inner
run_prefix_success_case write_tree_missing_ok_prefix src --missing-ok
run_prefix_first_success_case write_tree_prefix_missing_ok src --missing-ok
run_prefix_missing_case write_tree_prefix_missing missing
run_prefix_missing_case write_tree_prefix_leading_slash /src
run_prefix_missing_case write_tree_prefix_dot_slash ./src
run_prefix_missing_case write_tree_prefix_dot_component src/.
run_prefix_missing_case write_tree_prefix_parent_component src/../src
run_prefix_missing_case write_tree_prefix_missing_trailing_slash nosuch/
run_default_invalid_case write_tree_prefix_missing_value 129 --prefix
run_default_invalid_case write_tree_missing_ok_rejects_value 129 --missing-ok=true
run_default_invalid_case write_tree_no_missing_ok_rejects_value 129 --no-missing-ok=true
run_default_invalid_case write_tree_no_prefix_rejects_value 129 --no-prefix=true
