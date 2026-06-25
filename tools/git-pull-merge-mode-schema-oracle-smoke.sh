#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/debug/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-pull-merge-mode.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

configure_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
}

compare_repo_state() {
  local git_client="$1"
  local zmin_client="$2"
  test "$("$GIT_BIN" -C "$zmin_client" rev-list --parents -1 HEAD | cut -d' ' -f2-)" = "$("$GIT_BIN" -C "$git_client" rev-list --parents -1 HEAD | cut -d' ' -f2-)"
  test "$("$GIT_BIN" -C "$zmin_client" cat-file -p HEAD^{tree})" = "$("$GIT_BIN" -C "$git_client" cat-file -p HEAD^{tree})"
  test "$("$GIT_BIN" -C "$zmin_client" log --format=%B -1 HEAD)" = "$("$GIT_BIN" -C "$git_client" log --format=%B -1 HEAD)"
  test "$("$GIT_BIN" -C "$zmin_client" status --porcelain=v1 --branch)" = "$("$GIT_BIN" -C "$git_client" status --porcelain=v1 --branch)"
  cmp -s "$git_client/.git/FETCH_HEAD" "$zmin_client/.git/FETCH_HEAD"
}

run_no_ff_case() {
  local root="$tmpdir/pull_no_ff"
  local source="$root/source"
  local git_repo="$root/git-repo"
  local zmin_repo="$root/zmin-repo"
  local git_exit=0
  local zmin_exit=0

  mkdir -p "$root"
  configure_repo "$source"
  printf 'base\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm base
  "$GIT_BIN" -C "$source" switch -q -c side
  printf 'side\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm side
  "$GIT_BIN" -C "$source" switch -q main
  "$GIT_BIN" clone -q "$source" "$git_repo"
  "$GIT_BIN" clone -q "$source" "$zmin_repo"
  "$GIT_BIN" -C "$git_repo" branch side origin/side
  "$GIT_BIN" -C "$zmin_repo" branch side origin/side

  set +e
  "$GIT_BIN" -C "$git_repo" pull --no-rebase --no-ff . side >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" pull --no-rebase --no-ff . side >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err" "$root/zmin.err"
  compare_repo_state "$git_repo" "$zmin_repo"
  printf 'pull_no_ff\texact\texit=%s\n' "$git_exit"
}

run_ff_case() {
  local root="$tmpdir/pull_ff"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  local git_exit=0
  local zmin_exit=0

  mkdir -p "$root"
  configure_repo "$source"
  printf 'base\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm base
  "$GIT_BIN" clone -q "$source" "$git_client"
  "$GIT_BIN" clone -q "$source" "$zmin_client"
  printf 'next\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm next

  set +e
  "$GIT_BIN" -C "$git_client" pull --ff --no-rebase origin main >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_client" pull --ff --no-rebase origin main >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err" "$root/zmin.err"
  test "$("$GIT_BIN" -C "$zmin_client" rev-parse HEAD)" = "$("$GIT_BIN" -C "$git_client" rev-parse HEAD)"
  test "$("$GIT_BIN" -C "$zmin_client" cat-file -p HEAD^{tree})" = "$("$GIT_BIN" -C "$git_client" cat-file -p HEAD^{tree})"
  test "$("$GIT_BIN" -C "$zmin_client" status --porcelain=v1 --branch)" = "$("$GIT_BIN" -C "$git_client" status --porcelain=v1 --branch)"
  cmp -s "$git_client/.git/FETCH_HEAD" "$zmin_client/.git/FETCH_HEAD"
  printf 'pull_ff\texact\texit=%s\n' "$git_exit"
}

run_ff_only_no_rebase_case() {
  local root="$tmpdir/pull_ff_only_no_rebase"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  local git_exit=0
  local zmin_exit=0

  mkdir -p "$root"
  configure_repo "$source"
  printf 'base\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm base
  "$GIT_BIN" clone -q "$source" "$git_client"
  "$GIT_BIN" clone -q "$source" "$zmin_client"
  printf 'next\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm next

  set +e
  "$GIT_BIN" -C "$git_client" pull --ff-only --no-rebase origin main >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_client" pull --ff-only --no-rebase origin main >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err" "$root/zmin.err"
  test "$("$GIT_BIN" -C "$zmin_client" rev-parse HEAD)" = "$("$GIT_BIN" -C "$git_client" rev-parse HEAD)"
  test "$("$GIT_BIN" -C "$zmin_client" cat-file -p HEAD^{tree})" = "$("$GIT_BIN" -C "$git_client" cat-file -p HEAD^{tree})"
  test "$("$GIT_BIN" -C "$zmin_client" status --porcelain=v1 --branch)" = "$("$GIT_BIN" -C "$git_client" status --porcelain=v1 --branch)"
  cmp -s "$git_client/.git/FETCH_HEAD" "$zmin_client/.git/FETCH_HEAD"
  printf 'pull_ff_only_no_rebase\texact\texit=%s\n' "$git_exit"
}

run_ff_ff_only_no_rebase_case() {
  local root="$tmpdir/pull_ff_ff_only_no_rebase"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  local git_exit=0
  local zmin_exit=0

  mkdir -p "$root"
  configure_repo "$source"
  printf 'base\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm base
  "$GIT_BIN" clone -q "$source" "$git_client"
  "$GIT_BIN" clone -q "$source" "$zmin_client"
  printf 'next\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm next

  set +e
  "$GIT_BIN" -C "$git_client" pull --ff --ff-only --no-rebase origin main >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_client" pull --ff --ff-only --no-rebase origin main >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err" "$root/zmin.err"
  test "$("$GIT_BIN" -C "$zmin_client" rev-parse HEAD)" = "$("$GIT_BIN" -C "$git_client" rev-parse HEAD)"
  test "$("$GIT_BIN" -C "$zmin_client" cat-file -p HEAD^{tree})" = "$("$GIT_BIN" -C "$git_client" cat-file -p HEAD^{tree})"
  test "$("$GIT_BIN" -C "$zmin_client" status --porcelain=v1 --branch)" = "$("$GIT_BIN" -C "$git_client" status --porcelain=v1 --branch)"
  cmp -s "$git_client/.git/FETCH_HEAD" "$zmin_client/.git/FETCH_HEAD"
  printf 'pull_ff_ff_only_no_rebase\texact\texit=%s\n' "$git_exit"
}

run_ff_no_rebase_local_case() {
  local root="$tmpdir/pull_ff_no_rebase_local"
  local source="$root/source"
  local git_repo="$root/git-repo"
  local zmin_repo="$root/zmin-repo"
  local git_exit=0
  local zmin_exit=0

  mkdir -p "$root"
  configure_repo "$source"
  printf 'base\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm base
  "$GIT_BIN" -C "$source" switch -q -c side
  printf 'side\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm side
  "$GIT_BIN" -C "$source" switch -q main
  "$GIT_BIN" clone -q "$source" "$git_repo"
  "$GIT_BIN" clone -q "$source" "$zmin_repo"
  "$GIT_BIN" -C "$git_repo" branch side origin/side >/dev/null
  "$GIT_BIN" -C "$zmin_repo" branch side origin/side >/dev/null

  set +e
  "$GIT_BIN" -C "$git_repo" pull --ff --no-rebase . side >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" pull --ff --no-rebase . side >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err" "$root/zmin.err"
  test "$("$GIT_BIN" -C "$zmin_repo" rev-parse HEAD)" = "$("$GIT_BIN" -C "$git_repo" rev-parse HEAD)"
  test "$("$GIT_BIN" -C "$zmin_repo" cat-file -p HEAD^{tree})" = "$("$GIT_BIN" -C "$git_repo" cat-file -p HEAD^{tree})"
  test "$("$GIT_BIN" -C "$zmin_repo" status --porcelain=v1 --branch)" = "$("$GIT_BIN" -C "$git_repo" status --porcelain=v1 --branch)"
  cmp -s "$git_repo/.git/FETCH_HEAD" "$zmin_repo/.git/FETCH_HEAD"
  printf 'pull_ff_no_rebase_local\texact\texit=%s\n' "$git_exit"
}

run_ff_only_no_ff_case() {
  local root="$tmpdir/pull_ff_only_no_ff"
  local source="$root/source"
  local git_repo="$root/git-repo"
  local zmin_repo="$root/zmin-repo"
  local git_exit=0
  local zmin_exit=0

  mkdir -p "$root"
  configure_repo "$source"
  printf 'base\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm base
  "$GIT_BIN" -C "$source" switch -q -c side
  printf 'side\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm side
  "$GIT_BIN" -C "$source" switch -q main
  "$GIT_BIN" clone -q "$source" "$git_repo"
  "$GIT_BIN" clone -q "$source" "$zmin_repo"
  "$GIT_BIN" -C "$git_repo" branch side origin/side
  "$GIT_BIN" -C "$zmin_repo" branch side origin/side

  set +e
  "$GIT_BIN" -C "$git_repo" pull --ff-only --no-ff --no-rebase . side >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" pull --ff-only --no-ff --no-rebase . side >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err" "$root/zmin.err"
  test "$("$GIT_BIN" -C "$zmin_repo" rev-list --parents -1 HEAD | cut -d' ' -f2-)" = "$("$GIT_BIN" -C "$git_repo" rev-list --parents -1 HEAD | cut -d' ' -f2-)"
  test "$("$GIT_BIN" -C "$zmin_repo" cat-file -p HEAD^{tree})" = "$("$GIT_BIN" -C "$git_repo" cat-file -p HEAD^{tree})"
  test "$("$GIT_BIN" -C "$zmin_repo" log --format=%B -1 HEAD)" = "$("$GIT_BIN" -C "$git_repo" log --format=%B -1 HEAD)"
  test "$("$GIT_BIN" -C "$zmin_repo" status --porcelain=v1 --branch)" = "$("$GIT_BIN" -C "$git_repo" status --porcelain=v1 --branch)"
  cmp -s "$git_repo/.git/FETCH_HEAD" "$zmin_repo/.git/FETCH_HEAD"
  printf 'pull_ff_only_no_ff\texact\texit=%s\n' "$git_exit"
}

run_ff_no_ff_no_rebase_local_case() {
  local root="$tmpdir/pull_ff_no_ff_no_rebase_local"
  local source="$root/source"
  local git_repo="$root/git-repo"
  local zmin_repo="$root/zmin-repo"
  local git_exit=0
  local zmin_exit=0

  mkdir -p "$root"
  configure_repo "$source"
  printf 'base\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm base
  "$GIT_BIN" -C "$source" switch -q -c side
  printf 'side\n' >"$source/file.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm side
  "$GIT_BIN" -C "$source" switch -q main
  "$GIT_BIN" clone -q "$source" "$git_repo"
  "$GIT_BIN" clone -q "$source" "$zmin_repo"
  "$GIT_BIN" -C "$git_repo" branch side origin/side >/dev/null
  "$GIT_BIN" -C "$zmin_repo" branch side origin/side >/dev/null

  set +e
  "$GIT_BIN" -C "$git_repo" pull --ff --no-ff --no-rebase . side >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" pull --ff --no-ff --no-rebase . side >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  test "$git_exit" = "0"
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err" "$root/zmin.err"
  test "$("$GIT_BIN" -C "$zmin_repo" rev-list --parents -1 HEAD | cut -d' ' -f2-)" = "$("$GIT_BIN" -C "$git_repo" rev-list --parents -1 HEAD | cut -d' ' -f2-)"
  test "$("$GIT_BIN" -C "$zmin_repo" cat-file -p HEAD^{tree})" = "$("$GIT_BIN" -C "$git_repo" cat-file -p HEAD^{tree})"
  test "$("$GIT_BIN" -C "$zmin_repo" log --format=%B -1 HEAD)" = "$("$GIT_BIN" -C "$git_repo" log --format=%B -1 HEAD)"
  test "$("$GIT_BIN" -C "$zmin_repo" status --porcelain=v1 --branch)" = "$("$GIT_BIN" -C "$git_repo" status --porcelain=v1 --branch)"
  cmp -s "$git_repo/.git/FETCH_HEAD" "$zmin_repo/.git/FETCH_HEAD"
  printf 'pull_ff_no_ff_no_rebase_local\texact\texit=%s\n' "$git_exit"
}

run_no_ff_case
run_ff_case
run_ff_only_no_rebase_case
run_ff_ff_only_no_rebase_case
run_ff_no_rebase_local_case
run_ff_only_no_ff_case
run_ff_no_ff_no_rebase_local_case
