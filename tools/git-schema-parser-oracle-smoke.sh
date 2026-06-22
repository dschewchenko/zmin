#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-schema-parser-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

run_in_empty_dirs() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_exit=0
  local zmin_exit=0

  mkdir "$git_work" "$zmin_work"
  set +e
  (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  cmp -s "$git_out" "$zmin_out"
  cmp -s "$git_err" "$zmin_err"
  test ! -e "$git_work/.git"
  test ! -e "$zmin_work/.git"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_in_repo_dirs() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_exit=0
  local zmin_exit=0

  mkdir "$git_work" "$zmin_work"
  "$GIT_BIN" -C "$git_work" init -q
  "$GIT_BIN" -C "$zmin_work" init -q
  set +e
  (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  cmp -s "$git_out" "$zmin_out"
  cmp -s "$git_err" "$zmin_err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf 'base\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add file.txt
  "$GIT_BIN" -C "$repo" commit -qm "base"
}

run_in_seed_repos() {
  local name="$1"
  shift
  local seed_work="$tmpdir/${name}.seed.work"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_refs="$tmpdir/${name}.git.refs"
  local zmin_refs="$tmpdir/${name}.zmin.refs"
  local git_config="$tmpdir/${name}.git.config"
  local zmin_config="$tmpdir/${name}.zmin.config"
  local git_exit=0
  local zmin_exit=0

  make_seed_repo "$seed_work"
  cp -R "$seed_work" "$git_work"
  cp -R "$seed_work" "$zmin_work"
  set +e
  (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  cmp -s "$git_out" "$zmin_out"
  cmp -s "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" for-each-ref --format='%(refname)%00%(objectname)' >"$git_refs"
  "$GIT_BIN" -C "$zmin_work" for-each-ref --format='%(refname)%00%(objectname)' >"$zmin_refs"
  cmp -s "$git_refs" "$zmin_refs"
  "$GIT_BIN" -C "$git_work" config --null --list >"$git_config"
  "$GIT_BIN" -C "$zmin_work" config --null --list >"$zmin_config"
  cmp -s "$git_config" "$zmin_config"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_in_empty_dirs init_object_format_invalid init --object-format=bogus
run_in_empty_dirs init_ref_format_invalid init --ref-format=bogus
run_in_repo_dirs config_file_missing_long config --file=/no/such/file user.name
run_in_repo_dirs config_file_missing_short config -f /no/such/file user.name
run_in_repo_dirs add_pathspec_from_file_missing_equals add --pathspec-from-file=/no/such/file
run_in_repo_dirs add_pathspec_from_file_missing_separate add --pathspec-from-file /no/such/file
run_in_seed_repos show_ref_quiet_verify_short show-ref -q --verify refs/heads/main
run_in_seed_repos show_ref_quiet_verify_long show-ref --quiet --verify refs/heads/main
run_in_seed_repos show_ref_exists_existing show-ref --exists refs/heads/main
run_in_seed_repos branch_no_abbrev_listing branch --no-abbrev
run_in_seed_repos branch_abbrev_listing branch --abbrev=8
run_in_seed_repos branch_sort_listing branch --sort=refname
run_in_seed_repos branch_no_sort_listing branch --no-sort
run_in_seed_repos branch_column_never_listing branch --column=never
run_in_seed_repos branch_no_create_reflog branch --no-create-reflog no_reflog_branch
run_in_seed_repos branch_create_reflog branch --create-reflog reflog_branch
run_in_seed_repos config_default_missing config --default fallback missing.key
run_in_seed_repos config_add_value config --add user.nick Nick
run_in_seed_repos config_unset_all_missing config --unset-all user.none
run_in_seed_repos config_worktree_read config --worktree user.name
