#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-rebase-interactive-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

editor="$tmpdir/sequence-editor.sh"
cat >"$editor" <<'SH'
#!/usr/bin/env bash
sed -i.bak 's/^pick /badcmd /' "$1"
SH
chmod +x "$editor"

commit_fixed() {
  local repo="$1"
  local message="$2"
  local date="$3"
  GIT_AUTHOR_NAME="Oracle" \
    GIT_AUTHOR_EMAIL="oracle@example.test" \
    GIT_AUTHOR_DATE="$date" \
    GIT_COMMITTER_NAME="Oracle" \
    GIT_COMMITTER_EMAIL="oracle@example.test" \
    GIT_COMMITTER_DATE="$date" \
    "$GIT_BIN" -C "$repo" commit -qm "$message"
}

seed_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.test"
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add -A
  commit_fixed "$repo" one "2030-01-01T00:00:00 +0000"
  printf 'two\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add -A
  commit_fixed "$repo" two "2030-01-02T00:00:00 +0000"
}

rebase_state() {
  local repo="$1"
  {
    "$GIT_BIN" -C "$repo" status --short
    test -d "$repo/.git/rebase-merge" && echo "rebase-merge=present" || echo "rebase-merge=missing"
    "$GIT_BIN" -C "$repo" rev-parse HEAD
  }
}

run_gap() {
  local name="rebase_interactive_long_invalid_todo"
  local git_repo="$tmpdir/$name.git"
  local zmin_repo="$tmpdir/$name.zmin"
  local git_exit=0
  local zmin_exit=0
  local stdout_match=0
  local stderr_match=0
  local state_match=0

  seed_repo "$git_repo"
  seed_repo "$zmin_repo"

  set +e
  GIT_SEQUENCE_EDITOR="$editor" "$GIT_BIN" -C "$git_repo" rebase --interactive HEAD~1 >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  (cd "$zmin_repo" && GIT_SEQUENCE_EDITOR="$editor" "$ZMIN_BIN" rebase --interactive HEAD~1) >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  rebase_state "$git_repo" >"$tmpdir/$name.git.state"
  rebase_state "$zmin_repo" >"$tmpdir/$name.zmin.state"

  cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out" && stdout_match=1
  cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err" && stderr_match=1
  cmp -s "$tmpdir/$name.git.state" "$tmpdir/$name.zmin.state" && state_match=1

  if [ "$git_exit" = "$zmin_exit" ] &&
    [ "$stdout_match" = 1 ] &&
    [ "$stderr_match" = 1 ] &&
    [ "$state_match" = 1 ]; then
    echo "$name unexpectedly matches stock Git; update the matrix row" >&2
    return 1
  fi

  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\tstdout_match=%s\tstderr_match=%s\tstate_match=%s\n' \
    "$name" "$git_exit" "$zmin_exit" "$stdout_match" "$stderr_match" "$state_match"
}

run_gap
