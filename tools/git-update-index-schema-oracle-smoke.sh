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

tmpdir="$(mktemp -d /tmp/zmin-update-index-schema-oracle.XXXXXX)"
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

seed_empty_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
}

seed_tracked_repo() {
  local repo="$1"
  seed_empty_repo "$repo"
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m one
}

seed_tracked_clean_repo() {
  local repo="$1"
  seed_tracked_repo "$repo"
}

seed_assume_unchanged_repo() {
  local repo="$1"
  seed_tracked_repo "$repo"
  "$GIT_BIN" -C "$repo" update-index --assume-unchanged a.txt
}

seed_skip_worktree_repo() {
  local repo="$1"
  seed_tracked_repo "$repo"
  "$GIT_BIN" -C "$repo" update-index --skip-worktree a.txt
}

run_case() {
  local name="$1"
  local seed_kind="$2"
  shift 2
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_index="$tmpdir/${name}.git.index"
  local zmin_index="$tmpdir/${name}.zmin.index"
  local git_flags="$tmpdir/${name}.git.flags"
  local zmin_flags="$tmpdir/${name}.zmin.flags"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_exit=0
  local zmin_exit=0

  "seed_${seed_kind}_repo" "$seed"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"

  if [[ "$seed_kind" == "empty" ]]; then
    printf 'new\n' >"$git_work/a.txt"
    printf 'new\n' >"$zmin_work/a.txt"
  elif [[ "$seed_kind" != "tracked_clean" ]]; then
    printf 'two\n' >"$git_work/a.txt"
    printf 'two\n' >"$zmin_work/a.txt"
  fi

  set +e
  "$GIT_BIN" -C "$git_work" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" ls-files --stage >"$git_index"
  "$GIT_BIN" -C "$zmin_work" ls-files --stage >"$zmin_index"
  compare_files index "$git_index" "$zmin_index"
  "$GIT_BIN" -C "$git_work" ls-files -v >"$git_flags"
  "$GIT_BIN" -C "$zmin_work" ls-files -v >"$zmin_flags"
  compare_files index_flags "$git_flags" "$zmin_flags"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files worktree_status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_stdin_case() {
  local name="$1"
  local input="$2"
  shift 2
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_index="$tmpdir/${name}.git.index"
  local zmin_index="$tmpdir/${name}.zmin.index"
  local git_flags="$tmpdir/${name}.git.flags"
  local zmin_flags="$tmpdir/${name}.zmin.flags"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_exit=0
  local zmin_exit=0

  seed_tracked_repo "$seed"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"
  printf 'two\n' >"$git_work/a.txt"
  printf 'two\n' >"$zmin_work/a.txt"

  set +e
  printf '%b' "$input" | "$GIT_BIN" -C "$git_work" "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  printf '%b' "$input" | "$ZMIN_BIN" -C "$zmin_work" "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" ls-files --stage >"$git_index"
  "$GIT_BIN" -C "$zmin_work" ls-files --stage >"$zmin_index"
  compare_files index "$git_index" "$zmin_index"
  "$GIT_BIN" -C "$git_work" ls-files -v >"$git_flags"
  "$GIT_BIN" -C "$zmin_work" ls-files -v >"$zmin_flags"
  compare_files index_flags "$git_flags" "$zmin_flags"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files worktree_status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_cacheinfo_case() {
  local name="$1"
  shift
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_index="$tmpdir/${name}.git.index"
  local zmin_index="$tmpdir/${name}.zmin.index"
  local git_flags="$tmpdir/${name}.git.flags"
  local zmin_flags="$tmpdir/${name}.zmin.flags"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_exit=0
  local zmin_exit=0

  seed_tracked_repo "$seed"
  local blob
  blob="$("$GIT_BIN" -C "$seed" hash-object -w a.txt)"
  local args=()
  for arg in "$@"; do
    args+=("${arg//__BLOB__/$blob}")
  done
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" update-index "${args[@]}" >"$git_out" 2>"$git_err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" update-index "${args[@]}" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" ls-files --stage >"$git_index"
  "$GIT_BIN" -C "$zmin_work" ls-files --stage >"$zmin_index"
  compare_files index "$git_index" "$zmin_index"
  "$GIT_BIN" -C "$git_work" ls-files -v >"$git_flags"
  "$GIT_BIN" -C "$zmin_work" ls-files -v >"$zmin_flags"
  compare_files index_flags "$git_flags" "$zmin_flags"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files worktree_status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_index_info_case() {
  local name="$1"
  local input_template="$2"
  shift 2
  local seed="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_index="$tmpdir/${name}.git.index"
  local zmin_index="$tmpdir/${name}.zmin.index"
  local git_flags="$tmpdir/${name}.git.flags"
  local zmin_flags="$tmpdir/${name}.zmin.flags"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_exit=0
  local zmin_exit=0

  seed_tracked_repo "$seed"
  local blob
  blob="$("$GIT_BIN" -C "$seed" hash-object -w a.txt)"
  local input="${input_template//__BLOB__/$blob}"
  cp -R "$seed" "$git_work"
  cp -R "$seed" "$zmin_work"

  set +e
  printf '%b' "$input" | "$GIT_BIN" -C "$git_work" update-index "$@" >"$git_out" 2>"$git_err"
  git_exit=$?
  printf '%b' "$input" | "$ZMIN_BIN" -C "$zmin_work" update-index "$@" >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" ls-files --stage >"$git_index"
  "$GIT_BIN" -C "$zmin_work" ls-files --stage >"$zmin_index"
  compare_files index "$git_index" "$zmin_index"
  "$GIT_BIN" -C "$git_work" ls-files -v >"$git_flags"
  "$GIT_BIN" -C "$zmin_work" ls-files -v >"$zmin_flags"
  compare_files index_flags "$git_flags" "$zmin_flags"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files worktree_status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case update_index_add_path empty update-index --add a.txt
run_case update_index_positional_path tracked update-index a.txt
run_case update_index_refresh tracked_clean update-index --refresh
run_case update_index_really_refresh tracked_clean update-index --really-refresh
run_case update_index_assume_unchanged tracked update-index --assume-unchanged a.txt
run_case update_index_no_assume_unchanged assume_unchanged update-index --no-assume-unchanged a.txt
run_case update_index_skip_worktree tracked update-index --skip-worktree a.txt
run_case update_index_no_skip_worktree skip_worktree update-index --no-skip-worktree a.txt
run_case update_index_remove tracked update-index --remove a.txt
run_case update_index_force_remove tracked update-index --force-remove a.txt
run_stdin_case update_index_stdin 'a.txt\n' update-index --stdin
run_stdin_case update_index_z_stdin 'a.txt\0' update-index -z --stdin
run_cacheinfo_case update_index_cacheinfo_add --add --cacheinfo '100644,__BLOB__,b.txt'
run_cacheinfo_case update_index_cacheinfo_split --add --cacheinfo 100644 __BLOB__ b.txt
run_cacheinfo_case update_index_replace_cacheinfo --replace --cacheinfo '100644,__BLOB__,a.txt'
run_index_info_case update_index_index_info_blob '100644 blob __BLOB__\tb.txt\n' --index-info
run_index_info_case update_index_index_info_stage '100644 __BLOB__ 0\tb.txt\n' --index-info
