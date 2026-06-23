#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-daemon-oracle.XXXXXX")"
children=()
cleanup() {
  for pid in "${children[@]}"; do
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
  rm -rf "$tmpdir"
}
trap cleanup EXIT

unused_port() {
  python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
}

wait_for_port() {
  local port="$1"
  python3 - "$port" <<'PY'
import socket
import sys
import time
port = int(sys.argv[1])
deadline = time.time() + 10
while time.time() < deadline:
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=0.2):
            sys.exit(0)
    except OSError:
        time.sleep(0.05)
sys.exit(1)
PY
}

make_remote_root() {
  local root="$1"
  mkdir "$root"
  "$GIT_BIN" -C "$root" init -q --bare remote.git
  "$GIT_BIN" -C "$root" init -q -b main work
  "$GIT_BIN" -C "$root/work" config user.name "Oracle"
  "$GIT_BIN" -C "$root/work" config user.email "oracle@example.com"
  printf 'hello\n' >"$root/work/a.txt"
  "$GIT_BIN" -C "$root/work" add a.txt
  "$GIT_BIN" -C "$root/work" commit -qm "base"
  "$GIT_BIN" -C "$root/work" remote add origin "$root/remote.git"
  "$GIT_BIN" -C "$root/work" push -q origin main
  "$GIT_BIN" -C "$root/remote.git" symbolic-ref HEAD refs/heads/main
  printf '' >"$root/remote.git/git-daemon-export-ok"
}

start_daemon() {
  local command="$1"
  local root="$2"
  local port="$3"
  local label="$4"
  shift 4
  "$command" daemon \
    --export-all \
    --listen=127.0.0.1 \
    "--port=$port" \
    "--base-path=$root" \
    "$@" \
    "$root" >"$tmpdir/$label.out" 2>"$tmpdir/$label.err" &
  local pid="$!"
  children+=("$pid")
  wait_for_port "$port"
}

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

run_server_case() {
  local name="$1"
  shift
  local git_port
  local zmin_port
  git_port="$(unused_port)"
  zmin_port="$(unused_port)"
  start_daemon "$GIT_BIN" "$base_root" "$git_port" "$name.git" "$@"
  start_daemon "$ZMIN_BIN" "$base_root" "$zmin_port" "$name.zmin" "$@"
  "$GIT_BIN" ls-remote "git://127.0.0.1:$git_port/remote.git" >"$tmpdir/$name.git.refs"
  "$GIT_BIN" ls-remote "git://127.0.0.1:$zmin_port/remote.git" >"$tmpdir/$name.zmin.refs"
  compare_files "$name refs" "$tmpdir/$name.git.refs" "$tmpdir/$name.zmin.refs"
  printf '%s\tok\n' "$name"
}

run_server_rejection_case() {
  local name="$1"
  shift
  local git_port
  local zmin_port
  local git_exit=0
  local zmin_exit=0
  git_port="$(unused_port)"
  zmin_port="$(unused_port)"
  start_daemon "$GIT_BIN" "$base_root" "$git_port" "$name.git" "$@"
  start_daemon "$ZMIN_BIN" "$base_root" "$zmin_port" "$name.zmin" "$@"
  set +e
  "$GIT_BIN" ls-remote "git://127.0.0.1:$git_port/remote.git" >"$tmpdir/$name.git.refs" 2>"$tmpdir/$name.git.refs.err"
  git_exit=$?
  "$GIT_BIN" ls-remote "git://127.0.0.1:$zmin_port/remote.git" >"$tmpdir/$name.zmin.refs" 2>"$tmpdir/$name.zmin.refs.err"
  zmin_exit=$?
  set -e
  test "$git_exit" = "$zmin_exit"
  compare_files "$name stdout" "$tmpdir/$name.git.refs" "$tmpdir/$name.zmin.refs"
  compare_files "$name stderr" "$tmpdir/$name.git.refs.err" "$tmpdir/$name.zmin.refs.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_inetd_case() {
  local name="daemon_inetd_unknown_service"
  local packet="$tmpdir/$name.request"
  python3 - >"$packet" <<'PY'
payload = b"git-foo /remote.git\0host=localhost\0"
import sys
sys.stdout.buffer.write(f"{len(payload)+4:04x}".encode() + payload)
PY
  set +e
  "$GIT_BIN" daemon --inetd --export-all "--base-path=$base_root" <"$packet" \
    >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  local git_exit=$?
  "$ZMIN_BIN" daemon --inetd --export-all "--base-path=$base_root" <"$packet" \
    >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  local zmin_exit=$?
  set -e
  test "$git_exit" = "$zmin_exit"
  compare_files "$name stdout" "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out"
  compare_files "$name stderr" "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

base_root="$tmpdir/root"
make_remote_root "$base_root"
pid_file="$tmpdir/daemon.pid"

run_server_case daemon_base_path
run_server_case daemon_base_path_relaxed --base-path-relaxed
run_server_case daemon_export_all
run_inetd_case
run_server_case daemon_init_timeout --init-timeout=3
run_server_case daemon_listen
run_server_case daemon_max_connections --max-connections=8
run_server_case daemon_pid_file "--pid-file=$pid_file"
test -s "$pid_file"
run_server_case daemon_port
run_server_case daemon_reuseaddr --reuseaddr
run_server_rejection_case daemon_strict_paths --strict-paths
run_server_case daemon_timeout --timeout=3
run_server_case daemon_verbose --verbose
run_server_case daemon_directories "$base_root"
