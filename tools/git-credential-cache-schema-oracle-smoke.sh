#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d /tmp/zmin-ccache.XXXXXX)"
cleanup() {
  set +e
  [ -n "${git_socket:-}" ] && "$GIT_BIN" credential-cache --socket="$git_socket" exit >/dev/null 2>&1
  [ -n "${zmin_socket:-}" ] && "$ZMIN_BIN" credential-cache --socket="$zmin_socket" exit >/dev/null 2>&1
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

run_with_stdin() {
  local bin="$1"
  local work="$2"
  local stdin_file="$3"
  local out_file="$4"
  local err_file="$5"
  shift 5
  (cd "$work" && "$bin" "$@") <"$stdin_file" >"$out_file" 2>"$err_file"
}

run_case() {
  local name="$1"
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local socket_dir="$tmpdir/${name}.sockets"
  local complete="$tmpdir/${name}.complete.stdin"
  local query="$tmpdir/${name}.query.stdin"
  local erase="$tmpdir/${name}.erase.stdin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"

  mkdir "$git_work" "$zmin_work" "$socket_dir"
  chmod 700 "$socket_dir"
  git_socket="$socket_dir/git.sock"
  zmin_socket="$socket_dir/zmin.sock"
  printf 'protocol=https\nhost=example.com\nusername=u\npassword=p\n\n' >"$complete"
  printf 'protocol=https\nhost=example.com\n\n' >"$query"
  printf 'protocol=https\nhost=example.com\nusername=u\n\n' >"$erase"

  run_with_stdin "$GIT_BIN" "$git_work" "$complete" "$git_out" "$git_err" \
    credential-cache --socket="$git_socket" --timeout=60 store
  run_with_stdin "$ZMIN_BIN" "$zmin_work" "$complete" "$zmin_out" "$zmin_err" \
    credential-cache --socket="$zmin_socket" --timeout=60 store
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"

  run_with_stdin "$GIT_BIN" "$git_work" "$query" "$git_out" "$git_err" \
    credential-cache --socket="$git_socket" --timeout=60 get
  run_with_stdin "$ZMIN_BIN" "$zmin_work" "$query" "$zmin_out" "$zmin_err" \
    credential-cache --socket="$zmin_socket" --timeout=60 get
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"

  run_with_stdin "$GIT_BIN" "$git_work" "$erase" "$git_out" "$git_err" \
    credential-cache --socket="$git_socket" --timeout=60 erase
  run_with_stdin "$ZMIN_BIN" "$zmin_work" "$erase" "$zmin_out" "$zmin_err" \
    credential-cache --socket="$zmin_socket" --timeout=60 erase
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"

  run_with_stdin "$GIT_BIN" "$git_work" "$query" "$git_out" "$git_err" \
    credential-cache --socket="$git_socket" --timeout=60 get
  run_with_stdin "$ZMIN_BIN" "$zmin_work" "$query" "$zmin_out" "$zmin_err" \
    credential-cache --socket="$zmin_socket" --timeout=60 get
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  printf '%s\tok\texit=0\n' "$name"
}

run_case credential_cache_timeout_actions
