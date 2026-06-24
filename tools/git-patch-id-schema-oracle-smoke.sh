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

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-patch-id-schema-oracle.XXXXXX")"
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

repo="$tmpdir/repo"
"$GIT_BIN" init -q -b main "$repo"
"$GIT_BIN" -C "$repo" config user.name Oracle
"$GIT_BIN" -C "$repo" config user.email oracle@example.com
printf 'one\n' >"$repo/a.txt"
"$GIT_BIN" -C "$repo" add a.txt
"$GIT_BIN" -C "$repo" commit -qm base
printf 'two\n' >"$repo/a.txt"
"$GIT_BIN" -C "$repo" diff >"$tmpdir/diff.patch"

run_case() {
  local name="$1"
  local expected_exit="$2"
  shift 2
  local git_exit=0
  local zmin_exit=0

  set +e
  "$GIT_BIN" -C "$repo" patch-id "$@" <"$tmpdir/diff.patch" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$repo" patch-id "$@" <"$tmpdir/diff.patch" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
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

run_case patch_id_stable_repeated 0 --stable --stable
run_case patch_id_unstable_repeated 0 --unstable --unstable
run_case patch_id_verbatim_repeated 0 --verbatim --verbatim
run_case patch_id_no_stable_rejected 129 --no-stable
run_case patch_id_no_unstable_rejected 129 --no-unstable
run_case patch_id_no_verbatim_rejected 129 --no-verbatim
run_case patch_id_stable_rejects_value 129 --stable=true
run_case patch_id_unstable_rejects_value 129 --unstable=true
run_case patch_id_verbatim_rejects_value 129 --verbatim=true
