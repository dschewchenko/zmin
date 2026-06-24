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

tmpdir="$(mktemp -d /tmp/zmin-commit-tree-schema-oracle.XXXXXX)"
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

seed_repo() {
  local bin="$1"
  local repo="$2"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\n' >"$repo/a.txt"
  "$bin" -C "$repo" add a.txt
}

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  local git_tree
  local zmin_tree
  local git_exit=0
  local zmin_exit=0

  seed_repo "$GIT_BIN" "$git_work"
  seed_repo "$ZMIN_BIN" "$zmin_work"
  printf 'file message\n' >"$git_work/message.txt"
  printf 'file message\n' >"$zmin_work/message.txt"
  git_tree="$("$GIT_BIN" -C "$git_work" write-tree)"
  zmin_tree="$("$ZMIN_BIN" -C "$zmin_work" write-tree)"
  test "$git_tree" = "$zmin_tree"

  set +e
  "$GIT_BIN" -C "$git_work" commit-tree "$git_tree" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" commit-tree "$zmin_tree" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" cat-file commit "$(cat "$git_out")" >"$git_commit"
  "$GIT_BIN" -C "$zmin_work" cat-file commit "$(cat "$zmin_out")" >"$zmin_commit"
  compare_files commit_object "$git_commit" "$zmin_commit"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_tree_after_first_arg_case() {
  local name="$1"
  local first_arg="$2"
  shift 2
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  local git_tree
  local zmin_tree
  local git_exit=0
  local zmin_exit=0

  seed_repo "$GIT_BIN" "$git_work"
  seed_repo "$ZMIN_BIN" "$zmin_work"
  printf 'file message\n' >"$git_work/message.txt"
  printf 'file message\n' >"$zmin_work/message.txt"
  git_tree="$("$GIT_BIN" -C "$git_work" write-tree)"
  zmin_tree="$("$ZMIN_BIN" -C "$zmin_work" write-tree)"
  test "$git_tree" = "$zmin_tree"

  set +e
  "$GIT_BIN" -C "$git_work" commit-tree "$first_arg" "$git_tree" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" commit-tree "$first_arg" "$zmin_tree" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" cat-file commit "$(cat "$git_out")" >"$git_commit"
  "$GIT_BIN" -C "$zmin_work" cat-file commit "$(cat "$zmin_out")" >"$zmin_commit"
  compare_files commit_object "$git_commit" "$zmin_commit"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_tree_after_two_args_case() {
  local name="$1"
  local first_arg="$2"
  local second_arg="$3"
  shift 3
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  local git_tree
  local zmin_tree
  local git_exit=0
  local zmin_exit=0

  seed_repo "$GIT_BIN" "$git_work"
  seed_repo "$ZMIN_BIN" "$zmin_work"
  printf 'file message\n' >"$git_work/message.txt"
  printf 'file message\n' >"$zmin_work/message.txt"
  git_tree="$("$GIT_BIN" -C "$git_work" write-tree)"
  zmin_tree="$("$ZMIN_BIN" -C "$zmin_work" write-tree)"
  test "$git_tree" = "$zmin_tree"

  set +e
  "$GIT_BIN" -C "$git_work" commit-tree "$first_arg" "$second_arg" "$git_tree" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" commit-tree "$first_arg" "$second_arg" "$zmin_tree" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" cat-file commit "$(cat "$git_out")" >"$git_commit"
  "$GIT_BIN" -C "$zmin_work" cat-file commit "$(cat "$zmin_out")" >"$zmin_commit"
  compare_files commit_object "$git_commit" "$zmin_commit"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_stdin_case() {
  local name="$1"
  local input="$2"
  shift 2
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  local git_tree
  local zmin_tree
  local git_exit=0
  local zmin_exit=0

  seed_repo "$GIT_BIN" "$git_work"
  seed_repo "$ZMIN_BIN" "$zmin_work"
  git_tree="$("$GIT_BIN" -C "$git_work" write-tree)"
  zmin_tree="$("$ZMIN_BIN" -C "$zmin_work" write-tree)"
  test "$git_tree" = "$zmin_tree"

  set +e
  printf '%s' "$input" | "$GIT_BIN" -C "$git_work" commit-tree "$git_tree" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  printf '%s' "$input" | "$ZMIN_BIN" -C "$zmin_work" commit-tree "$zmin_tree" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" cat-file commit "$(cat "$git_out")" >"$git_commit"
  "$GIT_BIN" -C "$zmin_work" cat-file commit "$(cat "$zmin_out")" >"$zmin_commit"
  compare_files commit_object "$git_commit" "$zmin_commit"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_invalid_case() {
  local name="$1"
  local expected_exit="$2"
  shift 2
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_tree
  local zmin_tree
  local git_exit=0
  local zmin_exit=0

  seed_repo "$GIT_BIN" "$git_work"
  seed_repo "$ZMIN_BIN" "$zmin_work"
  git_tree="$("$GIT_BIN" -C "$git_work" write-tree)"
  zmin_tree="$("$ZMIN_BIN" -C "$zmin_work" write-tree)"
  test "$git_tree" = "$zmin_tree"

  set +e
  "$GIT_BIN" -C "$git_work" commit-tree "$git_tree" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" commit-tree "$zmin_tree" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$expected_exit"
  test "$zmin_exit" = "$expected_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_no_tree_invalid_case() {
  local name="$1"
  local expected_exit="$2"
  shift 2
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$GIT_BIN" "$git_work"
  seed_repo "$ZMIN_BIN" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" commit-tree "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" commit-tree "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$expected_exit"
  test "$zmin_exit" = "$expected_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_tree_argument_case() {
  local name="$1"
  local mode="$2"
  local expected_exit="$3"
  shift 3
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  local git_tree
  local zmin_tree
  local git_arg
  local zmin_arg
  local git_exit=0
  local zmin_exit=0

  seed_repo "$GIT_BIN" "$git_work"
  seed_repo "$ZMIN_BIN" "$zmin_work"
  git_tree="$("$GIT_BIN" -C "$git_work" write-tree)"
  zmin_tree="$("$ZMIN_BIN" -C "$zmin_work" write-tree)"
  test "$git_tree" = "$zmin_tree"
  case "$mode" in
    abbreviated)
      git_arg="${git_tree:0:8}"
      zmin_arg="${zmin_tree:0:8}"
      ;;
    blob)
      git_arg="$("$GIT_BIN" -C "$git_work" hash-object -w a.txt)"
      zmin_arg="$("$GIT_BIN" -C "$zmin_work" hash-object -w a.txt)"
      ;;
  esac

  set +e
  "$GIT_BIN" -C "$git_work" commit-tree "$git_arg" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" commit-tree "$zmin_arg" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$expected_exit"
  test "$zmin_exit" = "$expected_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  if test "$expected_exit" = 0; then
    "$GIT_BIN" -C "$git_work" cat-file commit "$(cat "$git_out")" >"$git_commit"
    "$GIT_BIN" -C "$zmin_work" cat-file commit "$(cat "$zmin_out")" >"$zmin_commit"
    compare_files commit_object "$git_commit" "$zmin_commit"
  fi
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_parent_case() {
  local name="$1"
  local mode="$2"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  local git_tree
  local zmin_tree
  local git_parent
  local zmin_parent
  local git_exit=0
  local zmin_exit=0

  seed_repo "$GIT_BIN" "$git_work"
  seed_repo "$ZMIN_BIN" "$zmin_work"
  git_tree="$("$GIT_BIN" -C "$git_work" write-tree)"
  zmin_tree="$("$ZMIN_BIN" -C "$zmin_work" write-tree)"
  test "$git_tree" = "$zmin_tree"
  git_parent="$("$GIT_BIN" -C "$git_work" commit-tree "$git_tree" -m root)"
  zmin_parent="$("$ZMIN_BIN" -C "$zmin_work" commit-tree "$zmin_tree" -m root)"
  test "$git_parent" = "$zmin_parent"

  set +e
  case "$mode" in
    attached)
      "$GIT_BIN" -C "$git_work" commit-tree "$git_tree" "-p$git_parent" -m child >"$git_out" 2>"$git_err"
      git_exit=$?
      "$ZMIN_BIN" -C "$zmin_work" commit-tree "$zmin_tree" "-p$zmin_parent" -m child >"$zmin_out" 2>"$zmin_err"
      zmin_exit=$?
      ;;
    message_first)
      "$GIT_BIN" -C "$git_work" commit-tree "$git_tree" -m child -p "$git_parent" >"$git_out" 2>"$git_err"
      git_exit=$?
      "$ZMIN_BIN" -C "$zmin_work" commit-tree "$zmin_tree" -m child -p "$zmin_parent" >"$zmin_out" 2>"$zmin_err"
      zmin_exit=$?
      ;;
    parent_first)
      "$GIT_BIN" -C "$git_work" commit-tree -p "$git_parent" "$git_tree" -m child >"$git_out" 2>"$git_err"
      git_exit=$?
      "$ZMIN_BIN" -C "$zmin_work" commit-tree -p "$zmin_parent" "$zmin_tree" -m child >"$zmin_out" 2>"$zmin_err"
      zmin_exit=$?
      ;;
    duplicate)
      "$GIT_BIN" -C "$git_work" commit-tree "$git_tree" -p "$git_parent" -p "$git_parent" -m child >"$git_out" 2>"$git_err"
      git_exit=$?
      "$ZMIN_BIN" -C "$zmin_work" commit-tree "$zmin_tree" -p "$zmin_parent" -p "$zmin_parent" -m child >"$zmin_out" 2>"$zmin_err"
      zmin_exit=$?
      ;;
  esac
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" cat-file commit "$(cat "$git_out")" >"$git_commit"
  "$GIT_BIN" -C "$zmin_work" cat-file commit "$(cat "$zmin_out")" >"$zmin_commit"
  compare_files commit_object "$git_commit" "$zmin_commit"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case commit_tree_positional_tree -m root
run_stdin_case commit_tree_empty_stdin ''
run_tree_argument_case commit_tree_abbreviated_tree abbreviated 0 -m root
run_tree_argument_case commit_tree_blob_tree blob 128 -m root
run_case commit_tree_attached_message -minline
run_tree_after_two_args_case commit_tree_message_before_tree -m root
run_case commit_tree_empty_message -m ''
run_case commit_tree_empty_message_then_message -m '' -m msg
run_case commit_tree_message_then_empty_message -m msg -m ''
run_case commit_tree_message_file -F message.txt
run_tree_after_two_args_case commit_tree_message_file_before_tree -F message.txt
run_case commit_tree_empty_message_file -F /dev/null
run_case commit_tree_attached_message_file -Fmessage.txt
run_stdin_case commit_tree_message_file_stdin 'stdin message
' -F -
run_case commit_tree_message_then_file -m inline -F message.txt
run_case commit_tree_file_then_message -F message.txt -m inline
run_case commit_tree_multiple_message_files -F message.txt -F message.txt
run_case commit_tree_no_gpg_sign --no-gpg-sign -m root
run_case commit_tree_repeated_no_gpg_sign --no-gpg-sign --no-gpg-sign -m root
run_tree_after_first_arg_case commit_tree_no_gpg_sign_before_tree --no-gpg-sign -m root
run_tree_after_two_args_case commit_tree_repeated_no_gpg_sign_before_tree --no-gpg-sign --no-gpg-sign -m root
run_parent_case commit_tree_attached_parent attached
run_parent_case commit_tree_message_before_parent message_first
run_parent_case commit_tree_parent_before_tree parent_first
run_parent_case commit_tree_duplicate_parent duplicate
run_invalid_case commit_tree_rejects_date 129 --date '2001-02-03T04:05:06+0000' -m root
run_invalid_case commit_tree_missing_message_file 128 -F missing.txt
run_invalid_case commit_tree_missing_parent 128 -p missing -m child
run_no_tree_invalid_case commit_tree_missing_tree_argument 128 -m root
run_invalid_case commit_tree_extra_tree_argument 128 -m root extra
