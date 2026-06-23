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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-maintenance-oracle.XXXXXX")"
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

normalize_config() {
  local input="$1"
  local output="$2"
  local repo_path="$3"
  local resolved_repo_path
  resolved_repo_path="$(cd "$repo_path" && pwd -P)"
  sed -e "s|$repo_path|<repo>|g" -e "s|$resolved_repo_path|<repo>|g" "$input" >"$output"
}

seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -q -m one
}

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_home="$tmpdir/${name}.git.home"
  local zmin_home="$tmpdir/${name}.zmin.home"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"
  mkdir "$git_home" "$zmin_home"

  set +e
  HOME="$git_home" "$GIT_BIN" -C "$git_work" maintenance unregister "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  HOME="$zmin_home" "$ZMIN_BIN" -C "$zmin_work" maintenance unregister "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  "$GIT_BIN" -C "$git_work" status --short >"$tmpdir/${name}.git.status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$tmpdir/${name}.zmin.status"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_gc_short_quiet_gap() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" maintenance run "$@" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" maintenance run "$@" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if test "$git_exit" = "$zmin_exit"; then
    echo "$name unexpectedly matches stock Git exit; update the open matrix row" >&2
    return 1
  fi
  test "$git_exit" = "129"
  test "$zmin_exit" = "0"
  grep -q "unknown switch \`q'" "$tmpdir/${name}.git.err"
  test ! -s "$tmpdir/${name}.zmin.err"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  "$GIT_BIN" -C "$git_work" status --short >"$tmpdir/${name}.git.status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$tmpdir/${name}.zmin.status"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  "$GIT_BIN" -C "$git_work" rev-parse --verify HEAD >"$tmpdir/${name}.git.head"
  "$GIT_BIN" -C "$zmin_work" rev-parse --verify HEAD >"$tmpdir/${name}.zmin.head"
  compare_files head "$tmpdir/${name}.git.head" "$tmpdir/${name}.zmin.head"
  printf '%s\tgap\tgit_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_config_file_case() {
  local name="$1"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_home="$tmpdir/${name}.git.home"
  local zmin_home="$tmpdir/${name}.zmin.home"
  local git_config="$git_home/custom.gitconfig"
  local zmin_config="$zmin_home/custom.gitconfig"
  local git_exit=0
  local zmin_exit=0

  seed_repo "$git_work"
  cp -R "$git_work" "$zmin_work"
  mkdir "$git_home" "$zmin_home"

  set +e
  HOME="$git_home" "$GIT_BIN" -C "$git_work" maintenance register --config-file "$git_config" >"$tmpdir/${name}.git.register.out" 2>"$tmpdir/${name}.git.register.err"
  git_exit=$?
  HOME="$zmin_home" "$ZMIN_BIN" -C "$zmin_work" maintenance register --config-file "$zmin_config" >"$tmpdir/${name}.zmin.register.out" 2>"$tmpdir/${name}.zmin.register.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files register-stdout "$tmpdir/${name}.git.register.out" "$tmpdir/${name}.zmin.register.out"
  compare_files register-stderr "$tmpdir/${name}.git.register.err" "$tmpdir/${name}.zmin.register.err"
  normalize_config "$git_config" "$tmpdir/${name}.git.register.config" "$git_work"
  normalize_config "$zmin_config" "$tmpdir/${name}.zmin.register.config" "$zmin_work"
  compare_files register-config "$tmpdir/${name}.git.register.config" "$tmpdir/${name}.zmin.register.config"

  set +e
  HOME="$git_home" "$GIT_BIN" -C "$git_work" maintenance unregister --config-file "$git_config" >"$tmpdir/${name}.git.unregister.out" 2>"$tmpdir/${name}.git.unregister.err"
  git_exit=$?
  HOME="$zmin_home" "$ZMIN_BIN" -C "$zmin_work" maintenance unregister --config-file "$zmin_config" >"$tmpdir/${name}.zmin.unregister.out" 2>"$tmpdir/${name}.zmin.unregister.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files unregister-stdout "$tmpdir/${name}.git.unregister.out" "$tmpdir/${name}.zmin.unregister.out"
  compare_files unregister-stderr "$tmpdir/${name}.git.unregister.err" "$tmpdir/${name}.zmin.unregister.err"
  normalize_config "$git_config" "$tmpdir/${name}.git.unregister.config" "$git_work"
  normalize_config "$zmin_config" "$tmpdir/${name}.zmin.unregister.config" "$zmin_work"
  compare_files unregister-config "$tmpdir/${name}.git.unregister.config" "$tmpdir/${name}.zmin.unregister.config"
  "$GIT_BIN" -C "$git_work" status --short >"$tmpdir/${name}.git.status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$tmpdir/${name}.zmin.status"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  printf '%s\tok\tregister_exit=0\tunregister_exit=0\n' "$name"
}

run_config_file_case maintenance_register_unregister_config_file_long
run_gc_short_quiet_gap maintenance_run_gc_quiet_short --task=gc -q
run_case maintenance_unregister_force_long --force
run_case maintenance_unregister_force_short -f
