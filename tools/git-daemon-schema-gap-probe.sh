#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-daemon-gap.XXXXXX")"
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
  "$command" daemon \
    --export-all \
    --listen=127.0.0.1 \
    "--port=$port" \
    "--base-path=$root" \
    --strict-paths \
    "$root" >"$tmpdir/$label.out" 2>"$tmpdir/$label.err" &
  local pid="$!"
  children+=("$pid")
  wait_for_port "$port"
}

root="$tmpdir/root"
make_remote_root "$root"
git_port="$(unused_port)"
zmin_port="$(unused_port)"
start_daemon "$GIT_BIN" "$root" "$git_port" git
start_daemon "$ZMIN_BIN" "$root" "$zmin_port" zmin

git_exit=0
zmin_exit=0
set +e
"$GIT_BIN" ls-remote "git://127.0.0.1:$git_port/remote.git" >"$tmpdir/git.refs" 2>"$tmpdir/git.refs.err"
git_exit=$?
"$GIT_BIN" ls-remote "git://127.0.0.1:$zmin_port/remote.git" >"$tmpdir/zmin.refs" 2>"$tmpdir/zmin.refs.err"
zmin_exit=$?
set -e

printf 'daemon_strict_paths\tstock_exit=%s\tzmin_exit=%s\n' "$git_exit" "$zmin_exit"
printf 'stock stderr:\n'
sed -n '1,4p' "$tmpdir/git.refs.err"
printf 'zmin stdout:\n'
sed -n '1,4p' "$tmpdir/zmin.refs"

test "$git_exit" = 128
test "$zmin_exit" = 0
