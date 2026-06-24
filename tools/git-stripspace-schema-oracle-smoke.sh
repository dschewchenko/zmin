#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-stripspace-schema-oracle.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

cat >"$tmpdir/input.txt" <<'EOF'
 subject

# comment
body

EOF

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

run_case() {
  local name="$1"
  local expected_exit="$2"
  shift 2
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" stripspace "$@" <"$tmpdir/input.txt" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" stripspace "$@" <"$tmpdir/input.txt" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if [ "$git_exit" != "$expected_exit" ] || [ "$zmin_exit" != "$expected_exit" ]; then
    echo "$name exit differs: expected=$expected_exit stock=$git_exit zmin=$zmin_exit" >&2
    return 1
  fi
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case stripspace_strip_comments_short_repeated 0 -s -s
run_case stripspace_comment_lines_short_repeated 0 -c -c
run_case stripspace_strip_comments_long_repeated 0 --strip-comments --strip-comments
run_case stripspace_comment_lines_long_repeated 0 --comment-lines --comment-lines
run_case stripspace_strip_comments_short_rejects_value 129 -s=true
run_case stripspace_comment_lines_short_rejects_value 129 -c=true
run_case stripspace_strip_comments_long_rejects_value 129 --strip-comments=true
run_case stripspace_comment_lines_long_rejects_value 129 --comment-lines=true
run_case stripspace_whitespace_rejected 129 --whitespace
