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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-add-oracle.XXXXXX")"
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

write_stable_index_debug() {
  local repo="$1"
  local out="$2"
  "$GIT_BIN" -C "$repo" ls-files --stage --debug \
    | sed -E '/^[[:space:]]+(ctime|mtime|dev|ino|uid|gid|size):/d' >"$out"
}

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf 'base\n' >"$repo/tracked.txt"
  mkdir "$repo/dir"
  printf 'old\n' >"$repo/dir/one.txt"
  printf '*.ignored\n' >"$repo/.gitignore"
  "$GIT_BIN" -C "$repo" add tracked.txt dir/one.txt .gitignore
  "$GIT_BIN" -C "$repo" commit -qm "base"
}

prepare_case() {
  local work="$1"
  local name="$2"
  cp -R "$base_seed" "$work"
  case "$name" in
    add_intent_long|add_intent_short|add_intent_repeated_long)
      printf 'intent\n' >"$work/intent.txt"
      ;;
    add_positional_path)
      printf 'new\n' >"$work/new.txt"
      ;;
    add_all_long|add_all_repeated_long)
      printf 'changed\n' >"$work/tracked.txt"
      printf 'new\n' >"$work/new.txt"
      rm "$work/dir/one.txt"
      ;;
    add_force_long|add_force_repeated_long)
      printf 'ignored\n' >"$work/force.ignored"
      ;;
    add_pathspec_file_nul)
      printf 'changed\n' >"$work/tracked.txt"
      printf 'two\n' >"$work/dir/two.txt"
      printf 'tracked.txt\0dir/two.txt\0' >"$work/paths.nul"
      ;;
    add_update_long|add_update_repeated_long)
      printf 'changed\n' >"$work/tracked.txt"
      printf 'new\n' >"$work/new.txt"
      rm "$work/dir/one.txt"
      ;;
    add_no_all_empty)
      ;;
    add_no_all_path|add_no_all_repeated_long|add_ignore_removal_long)
      printf 'new\n' >"$work/new.txt"
      rm "$work/tracked.txt"
      ;;
    add_no_ignore_removal_long)
      printf 'new\n' >"$work/new.txt"
      rm "$work/tracked.txt"
      ;;
    add_renormalize_long|add_renormalize_repeated_long|add_no_renormalize_long)
      printf 'changed\n' >"$work/tracked.txt"
      printf 'new\n' >"$work/new.txt"
      ;;
    add_dry_run_short|add_dry_run_repeated_long)
      printf 'dry\n' >"$work/dry.txt"
      ;;
    add_no_dry_run_long|add_no_dry_run_repeated_long)
      printf 'real\n' >"$work/real.txt"
      ;;
    add_verbose_long|add_verbose_repeated_long|add_verbose_short)
      printf 'verbose\n' >"$work/verbose.txt"
      ;;
    add_no_verbose_long|add_no_verbose_repeated_long)
      printf 'quiet\n' >"$work/quiet.txt"
      ;;
    add_no_update_long|add_no_update_repeated_long)
      printf 'new\n' >"$work/new.txt"
      ;;
    add_no_intent_to_add_long|add_no_intent_to_add_repeated_long)
      printf 'full\n' >"$work/full.txt"
      ;;
    add_no_refresh_long|add_no_refresh_repeated_long)
      printf 'fresh\n' >"$work/fresh.txt"
      ;;
    add_no_ignore_errors_long|add_no_ignore_errors_repeated_long|add_ignore_errors_repeated_long)
      printf 'errors-off\n' >"$work/errors-off.txt"
      ;;
    add_no_ignore_missing_long|add_no_ignore_missing_repeated_long)
      printf 'missing-off\n' >"$work/missing-off.txt"
      ;;
    add_no_pathspec_file_nul_long|add_no_pathspec_file_nul_repeated_long)
      printf 'lf-pathspec\n' >"$work/lf-pathspec.txt"
      ;;
    add_no_chmod_long|add_no_chmod_repeated_long)
      printf 'mode-default\n' >"$work/mode-default.txt"
      ;;
    add_sparse_long|add_sparse_repeated_long)
      printf 'sparse-ok\n' >"$work/sparse-ok.txt"
      ;;
    add_no_sparse_long|add_no_sparse_repeated_long)
      printf 'sparse-off\n' >"$work/sparse-off.txt"
      ;;
    add_no_pathspec_from_file_long|add_no_pathspec_from_file_repeated_long)
      printf 'pathspec-default\n' >"$work/pathspec-default.txt"
      ;;
    add_no_warn_embedded_repo_long)
      "$GIT_BIN" -C "$work" init -q inner
      "$GIT_BIN" -C "$work/inner" config user.name "Inner"
      "$GIT_BIN" -C "$work/inner" config user.email "inner@example.com"
      printf 'inner\n' >"$work/inner/file.txt"
      "$GIT_BIN" -C "$work/inner" add file.txt
      "$GIT_BIN" -C "$work/inner" commit -qm "inner"
      ;;
  esac
}

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_index="$tmpdir/${name}.git.index"
  local zmin_index="$tmpdir/${name}.zmin.index"
  local git_exit=0
  local zmin_exit=0

  prepare_case "$git_work" "$name"
  prepare_case "$zmin_work" "$name"

  set +e
  (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files status "$git_status" "$zmin_status"
  write_stable_index_debug "$git_work" "$git_index"
  write_stable_index_debug "$zmin_work" "$zmin_index"
  compare_files index "$git_index" "$zmin_index"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

base_seed="$tmpdir/base"
make_seed_repo "$base_seed"

run_case add_intent_long add --intent-to-add intent.txt
run_case add_intent_repeated_long add --intent-to-add --intent-to-add intent.txt
run_case add_intent_short add -N intent.txt
run_case add_positional_path add new.txt
run_case add_all_long add --all
run_case add_all_repeated_long add --all --all
run_case add_force_long add --force force.ignored
run_case add_force_repeated_long add --force --force force.ignored
run_case add_pathspec_file_nul add --pathspec-from-file=paths.nul --pathspec-file-nul
run_case add_update_long add --update
run_case add_update_repeated_long add --update --update
run_case add_no_all_empty add --no-all
run_case add_no_all_path add --no-all .
run_case add_no_all_repeated_long add --no-all --no-all .
run_case add_ignore_removal_long add --ignore-removal .
run_case add_no_ignore_removal_long add --no-ignore-removal .
run_case add_renormalize_long add --renormalize .
run_case add_renormalize_repeated_long add --renormalize --renormalize .
run_case add_no_renormalize_long add --no-renormalize .
run_case add_dry_run_short add -n dry.txt
run_case add_dry_run_repeated_long add --dry-run --dry-run dry.txt
run_case add_no_dry_run_long add --no-dry-run real.txt
run_case add_no_dry_run_repeated_long add --no-dry-run --no-dry-run real.txt
run_case add_verbose_long add --verbose verbose.txt
run_case add_verbose_repeated_long add --verbose --verbose verbose.txt
run_case add_verbose_short add -v verbose.txt
run_case add_no_verbose_long add --no-verbose quiet.txt
run_case add_no_verbose_repeated_long add --no-verbose --no-verbose quiet.txt
run_case add_no_update_long add --no-update new.txt
run_case add_no_update_repeated_long add --no-update --no-update new.txt
run_case add_no_intent_to_add_long add --no-intent-to-add full.txt
run_case add_no_intent_to_add_repeated_long add --no-intent-to-add --no-intent-to-add full.txt
run_case add_no_refresh_long add --no-refresh fresh.txt
run_case add_no_refresh_repeated_long add --no-refresh --no-refresh fresh.txt
run_case add_no_ignore_errors_long add --no-ignore-errors errors-off.txt
run_case add_no_ignore_errors_repeated_long add --no-ignore-errors --no-ignore-errors errors-off.txt
run_case add_ignore_errors_repeated_long add --ignore-errors --ignore-errors errors-off.txt
run_case add_no_ignore_missing_long add --no-ignore-missing missing-off.txt
run_case add_no_ignore_missing_repeated_long add --no-ignore-missing --no-ignore-missing missing-off.txt
run_case add_ignore_missing_repeated_long add --ignore-missing --ignore-missing --dry-run missing.txt
run_case add_no_pathspec_file_nul_long add --no-pathspec-file-nul lf-pathspec.txt
run_case add_no_pathspec_file_nul_repeated_long add --no-pathspec-file-nul --no-pathspec-file-nul lf-pathspec.txt
run_case add_no_chmod_long add --no-chmod mode-default.txt
run_case add_no_chmod_repeated_long add --no-chmod --no-chmod mode-default.txt
run_case add_sparse_long add --sparse sparse-ok.txt
run_case add_sparse_repeated_long add --sparse --sparse sparse-ok.txt
run_case add_no_sparse_long add --no-sparse sparse-off.txt
run_case add_no_sparse_repeated_long add --no-sparse --no-sparse sparse-off.txt
run_case add_no_pathspec_from_file_long add --no-pathspec-from-file pathspec-default.txt
run_case add_no_pathspec_from_file_repeated_long add --no-pathspec-from-file --no-pathspec-from-file pathspec-default.txt
run_case add_no_warn_embedded_repo_long add --no-warn-embedded-repo inner
