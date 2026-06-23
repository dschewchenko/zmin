#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-foreign-helper-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

run_probe() {
  local command="$1"
  local arg="$2"
  local name="$3"
  local git_exit=0
  local zmin_exit=0

  set +e
  (
    cd "$tmpdir"
    "$GIT_BIN" "$command" "$arg"
  ) >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (
    cd "$tmpdir"
    "$ZMIN_BIN" "$command" "$arg"
  ) >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  case "$name" in
    cvsexportcommit_positional_unknown_gap)
      test "$git_exit" = 1
      test "$zmin_exit" = 128
      grep -F "git: 'cvsexportcommit' is not a git command" "$tmpdir/${name}.git.err" >/dev/null
      grep -F "fatal: not a git repository" "$tmpdir/${name}.zmin.err" >/dev/null
      ;;
    cvsimport_positional_unknown_gap)
      test "$git_exit" = 1
      test "$zmin_exit" = 1
      grep -F "git: 'cvsimport' is not a git command" "$tmpdir/${name}.git.err" >/dev/null
      grep -F "No such file or directory" "$tmpdir/${name}.zmin.err" >/dev/null
      ;;
    gui_positional_unknown_gap)
      test "$git_exit" = 1
      test "$zmin_exit" = 129
      grep -F "git: 'gui' is not a git command" "$tmpdir/${name}.git.err" >/dev/null
      grep -F "fatal: unsupported gui command 'unknown'" "$tmpdir/${name}.zmin.err" >/dev/null
      ;;
    help_positional_unknown_gap)
      test "$git_exit" = 1
      test "$zmin_exit" = 2
      grep -F "No manual entry for gitunknown" "$tmpdir/${name}.git.err" >/dev/null
      grep -F "error: unrecognized subcommand 'unknown'" "$tmpdir/${name}.zmin.err" >/dev/null
      ;;
    svn_positional_unknown_gap)
      test "$git_exit" = 1
      test "$zmin_exit" = 129
      grep -F "git: 'svn' is not a git command" "$tmpdir/${name}.git.err" >/dev/null
      grep -F "fatal: unsupported svn command 'unknown'" "$tmpdir/${name}.zmin.err" >/dev/null
      ;;
    *)
      echo "unknown probe case: $name" >&2
      return 1
      ;;
  esac

  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_probe cvsexportcommit unknown cvsexportcommit_positional_unknown_gap
run_probe cvsimport unknown cvsimport_positional_unknown_gap
run_probe gui unknown gui_positional_unknown_gap
run_probe help unknown help_positional_unknown_gap
run_probe svn unknown svn_positional_unknown_gap
