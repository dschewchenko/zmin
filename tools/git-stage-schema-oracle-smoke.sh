#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
export GIT_EDITOR=:
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-stage-oracle.XXXXXX")"
cleanup() {
  chmod -R u+rwX "$tmpdir" 2>/dev/null || true
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
  printf 'mode\n' >"$repo/mode.txt"
  mkdir "$repo/dir"
  printf 'old\n' >"$repo/dir/one.txt"
  printf '*.ignored\n' >"$repo/.gitignore"
  "$GIT_BIN" -C "$repo" add tracked.txt mode.txt dir/one.txt .gitignore
  "$GIT_BIN" -C "$repo" commit -qm "base"
}

make_inner_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf 'inner\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add file.txt
  GIT_AUTHOR_DATE="2001-02-03T04:05:06Z" \
    GIT_COMMITTER_DATE="2001-02-03T04:05:06Z" \
    "$GIT_BIN" -C "$repo" commit -qm "inner"
}

prepare_case() {
  local work="$1"
  local name="$2"
  cp -R "$base_seed" "$work"
  case "$name" in
    stage_all_long|stage_all_repeated_long|stage_all_short|stage_all_repeated_short)
      printf 'changed\n' >"$work/tracked.txt"
      printf 'new\n' >"$work/new.txt"
      rm "$work/dir/one.txt"
      ;;
    stage_no_all_empty)
      ;;
    stage_no_all_path|stage_no_all_repeated_long|stage_ignore_removal_long)
      printf 'new\n' >"$work/new.txt"
      rm "$work/dir/one.txt"
      ;;
    stage_no_ignore_removal_long)
      printf 'new\n' >"$work/new.txt"
      rm "$work/dir/one.txt"
      ;;
    stage_chmod_long|stage_chmod_repeated_plus|stage_chmod_plus_then_minus|stage_chmod_minus_then_plus|stage_invalid_chmod_value)
      ;;
    stage_no_chmod_long|stage_no_chmod_repeated_long)
      printf 'mode default\n' >"$work/mode-default.txt"
      ;;
    stage_dry_run_long|stage_dry_run_repeated_long|stage_dry_run_short)
      printf 'dry\n' >"$work/dry.txt"
      ;;
    stage_dry_run_repeated|stage_dry_run_verbose_short|stage_dry_run_verbose_long)
      printf 'new\n' >"$work/new.txt"
      ;;
    stage_no_dry_run_long|stage_no_dry_run_repeated_long)
      printf 'real\n' >"$work/real.txt"
      ;;
    stage_force_long|stage_force_short)
      printf 'ignored\n' >"$work/force.ignored"
      ;;
    stage_force_repeated|stage_force_dry_run)
      printf 'ignored\n' >"$work/force.ignored"
      ;;
    stage_edit_noop|stage_edit_short_noop|stage_edit_repeated|stage_edit_short_repeated)
      printf 'changed\n' >"$work/tracked.txt"
      ;;
    stage_no_ignore_errors_long|stage_no_ignore_errors_repeated_long|stage_ignore_errors_repeated_long)
      printf 'errors off\n' >"$work/errors-off.txt"
      ;;
    stage_ignore_missing_long|stage_ignore_missing_repeated_long)
      printf 'changed\n' >"$work/tracked.txt"
      ;;
    stage_no_ignore_missing_long|stage_no_ignore_missing_repeated_long)
      printf 'missing off\n' >"$work/missing-off.txt"
      ;;
    stage_intent_long|stage_intent_repeated_long|stage_intent_short|stage_intent_repeated_short)
      printf 'intent\n' >"$work/intent.txt"
      ;;
    stage_no_intent_to_add_long|stage_no_intent_to_add_repeated_long)
      printf 'full\n' >"$work/full.txt"
      ;;
    stage_no_pathspec_file_nul_long|stage_no_pathspec_file_nul_repeated_long)
      printf 'lf\n' >"$work/lf-pathspec.txt"
      ;;
    stage_pathspec_file_nul)
      printf 'changed\n' >"$work/tracked.txt"
      printf 'two\n' >"$work/dir/two.txt"
      printf 'tracked.txt\0dir/two.txt\0' >"$work/paths.nul"
      ;;
    stage_pathspec_from_file)
      printf 'changed\n' >"$work/tracked.txt"
      printf 'two\n' >"$work/dir/two.txt"
      printf 'tracked.txt\ndir/two.txt\n' >"$work/paths.txt"
      ;;
    stage_no_pathspec_from_file_long|stage_no_pathspec_from_file_repeated_long)
      printf 'pathspec default\n' >"$work/pathspec-default.txt"
      ;;
    stage_positional_path)
      printf 'new\n' >"$work/new.txt"
      ;;
    stage_refresh_long)
      printf 'changed\n' >"$work/tracked.txt"
      ;;
    stage_no_refresh_long|stage_no_refresh_repeated_long)
      printf 'fresh\n' >"$work/fresh.txt"
      ;;
    stage_renormalize_long|stage_renormalize_repeated_long)
      printf 'changed\n' >"$work/tracked.txt"
      printf 'new\n' >"$work/new.txt"
      ;;
    stage_no_renormalize_long|stage_no_renormalize_repeated_long)
      printf 'renormalize off\n' >"$work/renormalize-off.txt"
      ;;
    stage_sparse_long|stage_sparse_repeated_long)
      printf 'sparse ok\n' >"$work/sparse-ok.txt"
      ;;
    stage_no_sparse_long|stage_no_sparse_repeated_long)
      printf 'sparse off\n' >"$work/sparse-off.txt"
      ;;
    stage_update_long|stage_update_repeated_long|stage_update_short|stage_update_repeated_short)
      printf 'changed\n' >"$work/tracked.txt"
      printf 'new\n' >"$work/new.txt"
      rm "$work/dir/one.txt"
      ;;
    stage_no_update_long|stage_no_update_repeated_long)
      printf 'new\n' >"$work/no-update.txt"
      ;;
    stage_verbose_long|stage_verbose_repeated_long|stage_verbose_short)
      printf 'verbose\n' >"$work/verbose.txt"
      ;;
    stage_verbose_repeated)
      printf 'new\n' >"$work/new.txt"
      ;;
    stage_no_verbose_long|stage_no_verbose_repeated_long)
      printf 'quiet\n' >"$work/quiet.txt"
      ;;
    stage_no_warn_embedded_repo_long)
      cp -R "$inner_seed" "$work/inner"
      ;;
    stage_invalid_short_a|stage_invalid_short_one|stage_invalid_short_two)
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

  if [ "$git_exit" != "$zmin_exit" ]; then
    echo "$name exit differs: stock=$git_exit zmin=$zmin_exit" >&2
    return 1
  fi
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
inner_seed="$tmpdir/inner-seed"
make_inner_repo "$inner_seed"

run_case stage_all_long stage --all
run_case stage_all_repeated_long stage --all --all
run_case stage_all_short stage -A
run_case stage_all_repeated_short stage -A -A
run_case stage_no_all_empty stage --no-all
run_case stage_no_all_path stage --no-all .
run_case stage_no_all_repeated_long stage --no-all --no-all .
run_case stage_ignore_removal_long stage --ignore-removal .
run_case stage_no_ignore_removal_long stage --no-ignore-removal .
run_case stage_chmod_long stage --chmod=+x mode.txt
run_case stage_chmod_repeated_plus stage --chmod=+x --chmod=+x mode.txt
run_case stage_chmod_plus_then_minus stage --chmod=+x --chmod=-x mode.txt
run_case stage_chmod_minus_then_plus stage --chmod=-x --chmod=+x mode.txt
run_case stage_no_chmod_long stage --no-chmod mode-default.txt
run_case stage_no_chmod_repeated_long stage --no-chmod --no-chmod mode-default.txt
run_case stage_dry_run_long stage --dry-run dry.txt
run_case stage_dry_run_repeated_long stage --dry-run --dry-run dry.txt
run_case stage_dry_run_short stage -n dry.txt
run_case stage_dry_run_repeated stage -n -n new.txt
run_case stage_dry_run_verbose_short stage -n -v new.txt
run_case stage_dry_run_verbose_long stage --dry-run --verbose new.txt
run_case stage_no_dry_run_long stage --no-dry-run real.txt
run_case stage_no_dry_run_repeated_long stage --no-dry-run --no-dry-run real.txt
run_case stage_force_long stage --force force.ignored
run_case stage_force_short stage -f force.ignored
run_case stage_force_repeated stage -f -f force.ignored
run_case stage_force_dry_run stage --force --dry-run force.ignored
run_case stage_edit_noop stage --edit
run_case stage_edit_short_noop stage -e
run_case stage_edit_repeated stage --edit --edit
run_case stage_edit_short_repeated stage -e -e
run_case stage_no_ignore_errors_long stage --no-ignore-errors errors-off.txt
run_case stage_no_ignore_errors_repeated_long stage --no-ignore-errors --no-ignore-errors errors-off.txt
run_case stage_ignore_errors_repeated_long stage --ignore-errors --ignore-errors errors-off.txt
run_case stage_ignore_missing_long stage --dry-run --ignore-missing tracked.txt missing.txt
run_case stage_ignore_missing_repeated_long stage --dry-run --ignore-missing --ignore-missing tracked.txt missing.txt
run_case stage_no_ignore_missing_long stage --no-ignore-missing missing-off.txt
run_case stage_no_ignore_missing_repeated_long stage --no-ignore-missing --no-ignore-missing missing-off.txt
run_case stage_intent_long stage --intent-to-add intent.txt
run_case stage_intent_repeated_long stage --intent-to-add --intent-to-add intent.txt
run_case stage_intent_short stage -N intent.txt
run_case stage_intent_repeated_short stage -N -N intent.txt
run_case stage_no_intent_to_add_long stage --no-intent-to-add full.txt
run_case stage_no_intent_to_add_repeated_long stage --no-intent-to-add --no-intent-to-add full.txt
run_case stage_pathspec_file_nul stage --pathspec-from-file=paths.nul --pathspec-file-nul
run_case stage_no_pathspec_file_nul_long stage --no-pathspec-file-nul lf-pathspec.txt
run_case stage_no_pathspec_file_nul_repeated_long stage --no-pathspec-file-nul --no-pathspec-file-nul lf-pathspec.txt
run_case stage_pathspec_from_file stage --pathspec-from-file=paths.txt
run_case stage_no_pathspec_from_file_long stage --no-pathspec-from-file pathspec-default.txt
run_case stage_no_pathspec_from_file_repeated_long stage --no-pathspec-from-file --no-pathspec-from-file pathspec-default.txt
run_case stage_positional_path stage new.txt
run_case stage_refresh_long stage --refresh tracked.txt
run_case stage_no_refresh_long stage --no-refresh fresh.txt
run_case stage_no_refresh_repeated_long stage --no-refresh --no-refresh fresh.txt
run_case stage_renormalize_long stage --renormalize .
run_case stage_renormalize_repeated_long stage --renormalize --renormalize .
run_case stage_no_renormalize_long stage --no-renormalize renormalize-off.txt
run_case stage_no_renormalize_repeated_long stage --no-renormalize --no-renormalize renormalize-off.txt
run_case stage_sparse_long stage --sparse sparse-ok.txt
run_case stage_sparse_repeated_long stage --sparse --sparse sparse-ok.txt
run_case stage_no_sparse_long stage --no-sparse sparse-off.txt
run_case stage_no_sparse_repeated_long stage --no-sparse --no-sparse sparse-off.txt
run_case stage_update_long stage --update
run_case stage_update_repeated_long stage --update --update
run_case stage_update_short stage -u
run_case stage_update_repeated_short stage -u -u
run_case stage_no_update_long stage --no-update no-update.txt
run_case stage_no_update_repeated_long stage --no-update --no-update no-update.txt
run_case stage_verbose_long stage --verbose verbose.txt
run_case stage_verbose_repeated_long stage --verbose --verbose verbose.txt
run_case stage_verbose_short stage -v verbose.txt
run_case stage_verbose_repeated stage -v -v new.txt
run_case stage_no_verbose_long stage --no-verbose quiet.txt
run_case stage_no_verbose_repeated_long stage --no-verbose --no-verbose quiet.txt
run_case stage_no_warn_embedded_repo_long stage --no-warn-embedded-repo inner
run_case stage_invalid_short_a stage -a
run_case stage_invalid_short_one stage -1
run_case stage_invalid_short_two stage -2
run_case stage_invalid_chmod_value stage --chmod=bad mode.txt
