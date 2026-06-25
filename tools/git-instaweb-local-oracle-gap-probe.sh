#!/usr/bin/env bash
set -euo pipefail

GIT_BIN="${GIT_BIN:-/usr/bin/git}"
tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-instaweb-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

src_root="$tmpdir/src"
curl -Ls https://github.com/git/git/archive/refs/tags/v2.47.1.tar.gz | tar -xz -C "$tmpdir"
src_root="$tmpdir/git-2.47.1"

perl_path="$(command -v perl)"
sh_path="$(command -v sh)"
python3 - <<'PY' "$src_root" "$tmpdir" "$perl_path" "$sh_path"
import pathlib, sys
src = pathlib.Path(sys.argv[1])
out = pathlib.Path(sys.argv[2])
repls = {
    "@@PERL@@": sys.argv[3],
    "@@GITWEBDIR@@": str(src / "gitweb"),
    "@SHELL_PATH@": sys.argv[4],
}
for name in ["git-instaweb.sh", "git-sh-setup.sh", "git-sh-i18n.sh"]:
    text = (src / name).read_text()
    for old, new in repls.items():
        text = text.replace(old, new)
    target = out / name.replace(".sh", "")
    target.write_text(text)
    target.chmod(0o755)
PY

mkdir -p "$tmpdir/bin"
cat >"$tmpdir/bin/lighttpd" <<'SH'
#!/bin/sh
conf="${2:-$1}"
pid_file="$(sed -n 's/^server.pid-file = "\(.*\)"$/\1/p' "$conf")"
port="$(sed -n 's/^server.port = \([0-9][0-9]*\)$/\1/p' "$conf")"
bind="$(sed -n 's/^server.bind = "\(.*\)"$/\1/p' "$conf")"
printf 'conf=%s\nport=%s\nbind=%s\n' "$conf" "$port" "$bind" > "$TMPDIR/lighttpd.log"
sleep 60 &
printf '%s\n' "$!" > "$pid_file"
SH
cat >"$tmpdir/bin/browser" <<'SH'
#!/bin/sh
printf '%s\n' "$*" > "$TMPDIR/browser.log"
SH
chmod +x "$tmpdir/bin/lighttpd" "$tmpdir/bin/browser"

export PATH="$tmpdir:$tmpdir/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export TMPDIR="$tmpdir"

repo="$tmpdir/repo"
mkdir -p "$repo"
cd "$repo"
"$GIT_BIN" init -q
"$GIT_BIN" config user.name "Test User"
"$GIT_BIN" config user.email "test@example.com"
printf 'hello\n' > README.md
"$GIT_BIN" add README.md
"$GIT_BIN" commit -qm init

set +e
"$GIT_BIN" instaweb -h >"$tmpdir/local-stock.out" 2>"$tmpdir/local-stock.err"
local_stock_exit=$?
"$tmpdir/git-instaweb" -h >"$tmpdir/upstream-help.out" 2>"$tmpdir/upstream-help.err"
upstream_help_exit=$?
"$tmpdir/git-instaweb" --daemon-internal --git-dir .git --work-tree "$repo" >"$tmpdir/daemon.out" 2>"$tmpdir/daemon.err"
daemon_exit=$?
"$tmpdir/git-instaweb" --start --local --port 12349 --httpd lighttpd --browser browser >"$tmpdir/start.out" 2>"$tmpdir/start.err"
start_exit=$?
start_pid_exists="$(test -f .git/pid && echo yes || echo no)"
start_browser_called="$(test -f "$tmpdir/browser.log" && echo yes || echo no)"
start_lighttpd_log="$(tr '\n' ';' < "$tmpdir/lighttpd.log")"
"$tmpdir/git-instaweb" --stop >"$tmpdir/stop.out" 2>"$tmpdir/stop.err"
stop_exit=$?
stop_pid_exists="$(test -f .git/pid && echo yes || echo no)"
rm -f "$tmpdir/browser.log" "$tmpdir/lighttpd.log"
"$tmpdir/git-instaweb" --start --httpd lighttpd --port 12348 > /dev/null 2>&1
"$tmpdir/git-instaweb" --restart -d lighttpd -b browser >"$tmpdir/restart.out" 2>"$tmpdir/restart.err"
restart_exit=$?
restart_pid_exists="$(test -f .git/pid && echo yes || echo no)"
restart_browser_called="$(test -f "$tmpdir/browser.log" && echo yes || echo no)"
restart_lighttpd_log="$(tr '\n' ';' < "$tmpdir/lighttpd.log")"
set -e

printf 'local_stock_git_instaweb_help\tstock_exit=%s\tstock_first_stderr=%s\n' \
  "$local_stock_exit" \
  "$(head -n 1 "$tmpdir/local-stock.err" | tr '\t' ' ')"
printf 'upstream_git_instaweb_help\tupstream_exit=%s\thelp_has_module_path=%s\thelp_has_browser=%s\n' \
  "$upstream_help_exit" \
  "$(grep -F -c -- '--[no-]module-path' "$tmpdir/upstream-help.out")" \
  "$(grep -F -c -- '--[no-]browser' "$tmpdir/upstream-help.out")"
printf 'upstream_git_instaweb_daemon_internal_rejected\texit=%s\tstderr_first=%s\n' \
  "$daemon_exit" \
  "$(head -n 1 "$tmpdir/daemon.err" | tr '\t' ' ')"
printf 'upstream_git_instaweb_start_shape\texit=%s\tstdout_bytes=%s\tstderr_bytes=%s\tpid_exists=%s\tbrowser_called=%s\tlighttpd_log=%s\n' \
  "$start_exit" \
  "$(wc -c < "$tmpdir/start.out" | tr -d ' ')" \
  "$(wc -c < "$tmpdir/start.err" | tr -d ' ')" \
  "$start_pid_exists" \
  "$start_browser_called" \
  "$start_lighttpd_log"
printf 'upstream_git_instaweb_stop_shape\texit=%s\tstdout_bytes=%s\tstderr_bytes=%s\tpid_exists=%s\n' \
  "$stop_exit" \
  "$(wc -c < "$tmpdir/stop.out" | tr -d ' ')" \
  "$(wc -c < "$tmpdir/stop.err" | tr -d ' ')" \
  "$stop_pid_exists"
printf 'upstream_git_instaweb_restart_shape\texit=%s\tstdout_bytes=%s\tstderr_bytes=%s\tpid_exists=%s\tbrowser_called=%s\tlighttpd_log=%s\n' \
  "$restart_exit" \
  "$(wc -c < "$tmpdir/restart.out" | tr -d ' ')" \
  "$(wc -c < "$tmpdir/restart.err" | tr -d ' ')" \
  "$restart_pid_exists" \
  "$restart_browser_called" \
  "$restart_lighttpd_log"
