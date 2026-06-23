#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-merge-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

seed_pair() {
  local name="$1"
  local diverge="$2"
  local source="$tmpdir/${name}.src"
  git_work="$tmpdir/${name}.git"
  zmin_work="$tmpdir/${name}.zmin"

  "$GIT_BIN" init -q -b main "$source"
  "$GIT_BIN" -C "$source" config user.name "Oracle"
  "$GIT_BIN" -C "$source" config user.email "oracle@example.test"
  printf 'base\n' >"$source/base.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm "base"
  "$GIT_BIN" -C "$source" checkout -q -b feature
  printf 'feature\n' >"$source/feature.txt"
  "$GIT_BIN" -C "$source" add -A
  "$GIT_BIN" -C "$source" commit -qm "feature"
  "$GIT_BIN" -C "$source" checkout -q main
  if [ "$diverge" = "diverge" ]; then
    printf 'main\n' >"$source/main.txt"
    "$GIT_BIN" -C "$source" add -A
    "$GIT_BIN" -C "$source" commit -qm "main"
  fi
  "$GIT_BIN" clone -q "$source" "$git_work"
  "$GIT_BIN" clone -q "$source" "$zmin_work"
  "$GIT_BIN" -C "$git_work" branch --quiet feature origin/feature
  "$GIT_BIN" -C "$zmin_work" branch --quiet feature origin/feature
  "$GIT_BIN" -C "$git_work" config user.name "Oracle"
  "$GIT_BIN" -C "$git_work" config user.email "oracle@example.test"
  "$GIT_BIN" -C "$zmin_work" config user.name "Oracle"
  "$GIT_BIN" -C "$zmin_work" config user.email "oracle@example.test"
}

side_effect_snapshot() {
  local repo="$1"
  local side_effect
  for side_effect in MERGE_HEAD MERGE_MSG MERGE_MODE SQUASH_MSG AUTO_MERGE; do
    if [ -e "$repo/.git/$side_effect" ]; then
      printf '%s\n' "$side_effect"
    fi
  done
}

repo_snapshot() {
  local repo="$1"
  {
    "$GIT_BIN" -C "$repo" rev-parse HEAD
    "$GIT_BIN" -C "$repo" rev-parse 'HEAD^{tree}'
    "$GIT_BIN" -C "$repo" status --porcelain=v1
    "$GIT_BIN" -C "$repo" log --format='%P|%s' -1
    side_effect_snapshot "$repo"
  }
}

run_gap() {
  local name="$1"
  local diverge="$2"
  shift 2
  local git_exit=0
  local zmin_exit=0

  seed_pair "$name" "$diverge"

  set +e
  "$GIT_BIN" -C "$git_work" merge "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" merge "$@") >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  repo_snapshot "$git_work" >"$tmpdir/${name}.git.snapshot"
  repo_snapshot "$zmin_work" >"$tmpdir/${name}.zmin.snapshot"
  if [ "$git_exit" = "$zmin_exit" ] &&
    cmp -s "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out" &&
    cmp -s "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err" &&
    cmp -s "$tmpdir/${name}.git.snapshot" "$tmpdir/${name}.zmin.snapshot"; then
    echo "$name unexpectedly matches stock Git; update the matrix row" >&2
    return 1
  fi
  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_gap merge_ff_only_long ff --ff-only feature
run_gap merge_no_ff_long ff --no-ff feature
run_gap merge_no_commit_long diverge --no-commit feature
run_gap merge_squash_long diverge --squash feature
run_gap merge_strategy_long diverge --strategy ort feature
run_gap merge_positional_commit ff feature
