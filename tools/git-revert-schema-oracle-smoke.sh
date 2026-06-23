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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-revert-oracle.XXXXXX")"
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

make_linear_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  "$GIT_BIN" -C "$repo" config commit.gpgsign false

  printf 'base\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m base
  printf 'changed\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m change
}

make_merge_seed_repo() {
  local repo="$1"
  local merge_commit_file="$2"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  "$GIT_BIN" -C "$repo" config commit.gpgsign false

  printf 'base\n' >"$repo/base.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" checkout -q -b side
  printf 'side\n' >"$repo/side.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m side
  "$GIT_BIN" -C "$repo" checkout -q main
  printf 'main\n' >"$repo/main.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m main
  "$GIT_BIN" -C "$repo" merge -q --no-ff side -m "merge side"
  "$GIT_BIN" -C "$repo" rev-parse HEAD >"$merge_commit_file"
}

record_repo_state() {
  local repo="$1"
  local prefix="$2"
  "$GIT_BIN" -C "$repo" rev-parse HEAD >"$prefix.head"
  "$GIT_BIN" -C "$repo" cat-file -p HEAD >"$prefix.commit"
  "$GIT_BIN" -C "$repo" cat-file -p HEAD^{tree} >"$prefix.tree"
  "$GIT_BIN" -C "$repo" status --short >"$prefix.status"
  "$GIT_BIN" -C "$repo" ls-files -s >"$prefix.index"
  for name in AUTO_MERGE COMMIT_EDITMSG MERGE_HEAD MERGE_MSG ORIG_HEAD REVERT_HEAD; do
    if test -e "$repo/.git/$name"; then
      printf '%s\n' "$name"
    fi
  done | sort >"$prefix.git-side-effects"
}

run_no_sequence_failure_case() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_linear_seed_repo "$seed"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" revert "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" revert "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "128"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  record_repo_state "$git_work" "$tmpdir/${name}.git"
  record_repo_state "$zmin_work" "$tmpdir/${name}.zmin"
  compare_files head "$tmpdir/${name}.git.head" "$tmpdir/${name}.zmin.head"
  compare_files tree "$tmpdir/${name}.git.tree" "$tmpdir/${name}.zmin.tree"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  compare_files index "$tmpdir/${name}.git.index" "$tmpdir/${name}.zmin.index"
  compare_files git-side-effects "$tmpdir/${name}.git.git-side-effects" "$tmpdir/${name}.zmin.git-side-effects"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_linear_case() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  make_linear_seed_repo "$seed"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" revert "$@" HEAD >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" revert "$@" HEAD >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"

  record_repo_state "$git_work" "$tmpdir/${name}.git"
  record_repo_state "$zmin_work" "$tmpdir/${name}.zmin"
  compare_files head "$tmpdir/${name}.git.head" "$tmpdir/${name}.zmin.head"
  compare_files commit "$tmpdir/${name}.git.commit" "$tmpdir/${name}.zmin.commit"
  compare_files tree "$tmpdir/${name}.git.tree" "$tmpdir/${name}.zmin.tree"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  compare_files index "$tmpdir/${name}.git.index" "$tmpdir/${name}.zmin.index"
  if cmp -s "$tmpdir/${name}.git.git-side-effects" "$tmpdir/${name}.zmin.git-side-effects"; then
    echo "$name unexpectedly matches stock Git side effects; update the open matrix row" >&2
    return 1
  fi
  grep -qx AUTO_MERGE "$tmpdir/${name}.git.git-side-effects"
  grep -qx MERGE_MSG "$tmpdir/${name}.git.git-side-effects"
  grep -qx REVERT_HEAD "$tmpdir/${name}.git.git-side-effects"
  ! grep -qx AUTO_MERGE "$tmpdir/${name}.zmin.git-side-effects"
  ! grep -qx MERGE_MSG "$tmpdir/${name}.zmin.git-side-effects"
  ! grep -qx REVERT_HEAD "$tmpdir/${name}.zmin.git-side-effects"
  compare_files worktree-file "$git_work/file.txt" "$zmin_work/file.txt"
  printf '%s\tgap\texit=%s\n' "$name" "$git_exit"
}

run_mainline_case() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local merge_commit_file="$tmpdir/${name}.merge-commit"
  local git_exit=0
  local zmin_exit=0

  make_merge_seed_repo "$seed" "$merge_commit_file"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" revert "$@" "$(cat "$merge_commit_file")" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" revert "$@" "$(cat "$merge_commit_file")" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  if cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"; then
    echo "$name unexpectedly matches stock Git stdout; update the open matrix row" >&2
    return 1
  fi

  record_repo_state "$git_work" "$tmpdir/${name}.git"
  record_repo_state "$zmin_work" "$tmpdir/${name}.zmin"
  compare_files tree "$tmpdir/${name}.git.tree" "$tmpdir/${name}.zmin.tree"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  compare_files index "$tmpdir/${name}.git.index" "$tmpdir/${name}.zmin.index"
  printf '%s\tgap\texit=%s\n' "$name" "$git_exit"
}

run_no_sequence_failure_case revert_abort_no_sequence --abort
run_no_sequence_failure_case revert_continue_no_sequence --continue
run_linear_case revert_no_commit_long --no-commit
run_linear_case revert_no_commit_short -n
run_mainline_case revert_mainline_long --mainline 1
