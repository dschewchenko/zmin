#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/debug/zmin}"
GIT_BIN="${GIT_BIN:-git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-replay-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

seed_source_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name Bench
  "$GIT_BIN" -C "$repo" config user.email bench@example.test
  "$GIT_BIN" -C "$repo" config commit.gpgsign false
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add -A
  GIT_AUTHOR_DATE='1700000000 +0000' GIT_COMMITTER_DATE='1700000000 +0000' \
    "$GIT_BIN" -C "$repo" commit -qm one
  base="$("$GIT_BIN" -C "$repo" rev-parse HEAD)"
  printf 'two\n' >"$repo/a.txt"
  GIT_AUTHOR_DATE='1700000010 +0000' GIT_COMMITTER_DATE='1700000010 +0000' \
    "$GIT_BIN" -C "$repo" commit -qam two
  tip="$("$GIT_BIN" -C "$repo" rev-parse HEAD)"
  range="$base..$tip"
}

prepare_case_repo() {
  local source="$1"
  local repo="$2"
  cp -R "$source" "$repo"
  "$GIT_BIN" -C "$repo" branch topic "$base"
}

run_case() {
  local name="$1"
  local mode="$2"
  shift 2
  local git_repo="$tmpdir/$name.git"
  local zmin_repo="$tmpdir/$name.zmin"
  prepare_case_repo "$source" "$git_repo"
  prepare_case_repo "$source" "$zmin_repo"

  local git_exit=0
  local zmin_exit=0
  set +e
  if [ "$mode" = "stdin" ]; then
    printf '%s\n' "$range" | GIT_EDITOR=true GIT_COMMITTER_DATE='1700000100 +0000' \
      "$GIT_BIN" -C "$git_repo" "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
    git_exit=$?
    (
      cd "$zmin_repo"
      printf '%s\n' "$range" | GIT_EDITOR=true GIT_COMMITTER_DATE='1700000100 +0000' \
        "$ZMIN_BIN" "$@"
    ) >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
    zmin_exit=$?
  else
    GIT_EDITOR=true GIT_COMMITTER_DATE='1700000100 +0000' \
      "$GIT_BIN" -C "$git_repo" "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
    git_exit=$?
    (
      cd "$zmin_repo"
      GIT_EDITOR=true GIT_COMMITTER_DATE='1700000100 +0000' \
        "$ZMIN_BIN" "$@"
    ) >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
    zmin_exit=$?
  fi
  set -e

  "$GIT_BIN" -C "$git_repo" show-ref --hash refs/heads/topic >"$tmpdir/$name.git.topic"
  "$GIT_BIN" -C "$zmin_repo" show-ref --hash refs/heads/topic >"$tmpdir/$name.zmin.topic"
  "$GIT_BIN" -C "$git_repo" status --short >"$tmpdir/$name.git.status"
  "$GIT_BIN" -C "$zmin_repo" status --short >"$tmpdir/$name.zmin.status"

  local stdout_match=0
  local stderr_match=0
  local topic_match=0
  local status_match=0
  cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out" && stdout_match=1
  cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err" && stderr_match=1
  cmp -s "$tmpdir/$name.git.topic" "$tmpdir/$name.zmin.topic" && topic_match=1
  cmp -s "$tmpdir/$name.git.status" "$tmpdir/$name.zmin.status" && status_match=1

  if [ "$git_exit" = "$zmin_exit" ] &&
    [ "$stdout_match" = 1 ] &&
    [ "$stderr_match" = 1 ] &&
    [ "$topic_match" = 1 ] &&
    [ "$status_match" = 1 ]; then
    printf '%s\texact\tstock_exit=%s\tzmin_exit=%s\tstdout_match=%s\tstderr_match=%s\ttopic_match=%s\tstatus_match=%s\n' \
      "$name" "$git_exit" "$zmin_exit" "$stdout_match" "$stderr_match" "$topic_match" "$status_match"
    return 0
  fi

  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\tstdout_match=%s\tstderr_match=%s\ttopic_match=%s\tstatus_match=%s\n' \
    "$name" "$git_exit" "$zmin_exit" "$stdout_match" "$stderr_match" "$topic_match" "$status_match"
  return 1
}

source="$tmpdir/source"
seed_source_repo "$source"

run_case replay_topo_order arg replay --topo-order --advance topic "$range"
run_case replay_date_order arg replay --date-order --advance topic "$range"
run_case replay_author_date_order arg replay --author-date-order --advance topic "$range"
run_case replay_reverse arg replay --reverse --advance topic "$range"
run_case replay_count arg replay --count --advance topic "$range"
run_case replay_alternate_refs arg replay --alternate-refs --advance topic "$range"
run_case replay_ignore_missing arg replay --ignore-missing --advance topic "$range"
run_case replay_indexed_objects arg replay --indexed-objects --advance topic "$range"
run_case replay_remove_empty arg replay --remove-empty --advance topic "$range"
run_case replay_single_worktree arg replay --single-worktree --advance topic "$range"
run_case replay_unpacked arg replay --unpacked --advance topic "$range"
run_case replay_first_parent arg replay --first-parent --advance topic "$range"
run_case replay_right_only arg replay --right-only --advance topic "$range"
run_case replay_left_right arg replay --left-right --advance topic "$range"
run_case replay_cherry arg replay --cherry --advance topic "$range"
run_case replay_cherry_pick arg replay --cherry-pick --advance topic "$range"
run_case replay_cherry_mark arg replay --cherry-mark --advance topic "$range"
run_case replay_parents arg replay --parents --advance topic "$range"
run_case replay_objects arg replay --objects --advance topic "$range"
run_case replay_objects_edge arg replay --objects-edge --advance topic "$range"
run_case replay_objects_edge_aggressive arg replay --objects-edge-aggressive --advance topic "$range"
run_case replay_show_signature arg replay --show-signature --advance topic "$range"
run_case replay_no_walk arg replay --no-walk --advance topic "$range"
run_case replay_no_merges arg replay --no-merges --advance topic "$range"
run_case replay_exclude_tags arg replay --exclude=refs/tags/\* --advance topic "$range"
run_case replay_author_bench arg replay --author=Bench --advance topic "$range"
run_case replay_committer_bench arg replay --committer=Bench --advance topic "$range"
run_case replay_since_epoch arg replay --since=1970-01-01 --advance topic "$range"
run_case replay_after_epoch arg replay --after=1970-01-01 --advance topic "$range"
run_case replay_until_future arg replay --until=2100-01-01 --advance topic "$range"
run_case replay_before_future arg replay --before=2100-01-01 --advance topic "$range"
run_case replay_abbrev_commit arg replay --abbrev-commit --advance topic "$range"
run_case replay_no_abbrev_commit arg replay --no-abbrev-commit --advance topic "$range"
run_case replay_encoding_utf8 arg replay --encoding=UTF-8 --advance topic "$range"
run_case replay_grep_two arg replay --grep=two --advance topic "$range"
run_case replay_regexp_ignore_case_short arg replay -i --grep=two --advance topic "$range"
run_case replay_regexp_ignore_case_long arg replay --regexp-ignore-case --grep=two --advance topic "$range"
run_case replay_fixed_strings_long arg replay --fixed-strings --grep=two --advance topic "$range"
run_case replay_fixed_strings_short arg replay -F --grep=two --advance topic "$range"
run_case replay_basic_regexp arg replay --basic-regexp --grep=two --advance topic "$range"
run_case replay_extended_regexp_long arg replay --extended-regexp --grep=two --advance topic "$range"
run_case replay_extended_regexp_short arg replay -E --grep=two --advance topic "$range"
run_case replay_perl_regexp_long arg replay --perl-regexp --grep=two --advance topic "$range"
run_case replay_perl_regexp_short arg replay -P --grep=two --advance topic "$range"
run_case replay_all_match arg replay --all-match --grep=two --advance topic "$range"
run_case replay_invert_grep arg replay --invert-grep --grep=missing --advance topic "$range"
run_case replay_dense arg replay --dense --advance topic "$range"
run_case replay_sparse arg replay --sparse --advance topic "$range"
run_case replay_full_history arg replay --full-history --advance topic "$range"
run_case replay_children arg replay --children --advance topic "$range"
run_case replay_ancestry_path arg replay --ancestry-path --advance topic "$range"
run_case replay_simplify_merges arg replay --simplify-merges --advance topic "$range"
run_case replay_simplify_by_decoration arg replay --simplify-by-decoration --advance topic "$range"
run_case replay_in_commit_order arg replay --in-commit-order --advance topic "$range"
run_case replay_expand_tabs arg replay --expand-tabs --advance topic "$range"
run_case replay_no_expand_tabs arg replay --no-expand-tabs --advance topic "$range"
run_case replay_show_linear_break arg replay --show-linear-break=bar --advance topic "$range"
run_case replay_notes arg replay --notes --advance topic "$range"
run_case replay_no_notes arg replay --no-notes --advance topic "$range"
run_case replay_show_notes arg replay --show-notes --advance topic "$range"
run_case replay_show_notes_by_default arg replay --show-notes-by-default --advance topic "$range"
run_case replay_standard_notes arg replay --standard-notes --advance topic "$range"
run_case replay_no_standard_notes arg replay --no-standard-notes --advance topic "$range"
run_case replay_do_walk arg replay --do-walk --advance topic "$range"
run_case replay_reflog arg replay --reflog --advance topic "$range"
run_case replay_bisect arg replay --bisect --advance topic "$range"
run_case replay_pretty_oneline arg replay --pretty=oneline --advance topic "$range"
run_case replay_oneline arg replay --advance topic --oneline "$range"
run_case replay_format_hash arg replay --format=%H --advance topic "$range"
run_case replay_date_iso arg replay --date=iso --advance topic "$range"
run_case replay_relative_date arg replay --relative-date --advance topic "$range"
run_case replay_quiet arg replay --quiet --advance topic "$range"
run_case replay_max_age_zero arg replay --max-age=0 --advance topic "$range"
run_case replay_max_parents_one arg replay --max-parents=1 --advance topic "$range"
run_case replay_no_max_parents arg replay --no-max-parents --advance topic "$range"
run_case replay_min_parents_zero arg replay --min-parents=0 --advance topic "$range"
run_case replay_no_min_parents arg replay --no-min-parents --advance topic "$range"
run_case replay_max_count_one arg replay --max-count=1 --advance topic "$range"
run_case replay_skip_zero arg replay --skip=0 --advance topic "$range"
run_case replay_since_as_filter arg replay --since-as-filter=1970-01-01 --advance topic "$range"
run_case replay_exclude_first_parent_only arg replay --exclude-first-parent-only --advance topic "$range"
run_case replay_exclude_hidden_fetch arg replay --exclude-hidden=fetch --advance topic "$range"
run_case replay_tags arg replay --tags --advance topic "$range"
run_case replay_remotes arg replay --remotes --advance topic "$range"
run_case replay_branches_multiple_sources arg replay --branches --advance topic "$range"
run_case replay_all_multiple_sources arg replay --all --advance topic "$range"
run_case replay_not_empty_selection arg replay --advance topic --not "$base" "$tip"
run_case replay_stdin stdin replay --stdin --advance topic
