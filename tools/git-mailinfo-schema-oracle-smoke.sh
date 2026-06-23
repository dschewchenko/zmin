#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-mailinfo-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

cat >"$tmpdir/mail.txt" <<'MAIL'
From: Alice <alice@example.com>
Subject: [PATCH] add greeting
Message-Id: <patch-1@example.com>
Content-Type: text/plain; charset=UTF-8

Commit body line.

---
 greeting.txt | 1 +
 1 file changed, 1 insertion(+)
 create mode 100644 greeting.txt

diff --git a/greeting.txt b/greeting.txt
new file mode 100644
index 0000000..ce01362
--- /dev/null
+++ b/greeting.txt
@@ -0,0 +1 @@
+hello
--
2.47.1
MAIL

run_oracle() {
  local name="$1"
  shift
  local root="$tmpdir/$name"
  local git_exit=0
  local zmin_exit=0
  mkdir "$root"

  set +e
  "$GIT_BIN" -C "$root" mailinfo "$@" git-msg git-patch <"$tmpdir/mail.txt" >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$root" mailinfo "$@" zmin-msg zmin-patch <"$tmpdir/mail.txt" >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  test "$git_exit" = 0
  test "$zmin_exit" = 0
  cmp -s "$root/git.out" "$root/zmin.out"
  cmp -s "$root/git.err" "$root/zmin.err"
  cmp -s "$root/git-msg" "$root/zmin-msg"
  cmp -s "$root/git-patch" "$root/zmin-patch"
}

run_oracle mailinfo_encoding --encoding UTF-8
run_oracle mailinfo_quoted_cr --quoted-cr nowarn
run_oracle mailinfo_scissors --scissors
run_oracle mailinfo_message_id_long --message-id
run_oracle mailinfo_no_recode -n
run_oracle mailinfo_recode -u
