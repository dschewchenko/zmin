#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-check-mailmap-oracle.XXXXXX")"
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

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  cat >"$repo/.mailmap" <<'EOF'
Proper Name <proper@example.com> Alias Name <alias@example.com>
<canonical@example.com> <old@example.com>
EOF
  cat >"$repo/alt.mailmap" <<'EOF'
Alt Name <alt@example.com> Alt Alias <alt-alias@example.com>
EOF
  cat >"$repo/blob.mailmap" <<'EOF'
Blob Name <blob@example.com> Blob Alias <blob-alias@example.com>
EOF
  "$GIT_BIN" -C "$repo" hash-object -w blob.mailmap >"$repo/blob.oid"
}

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_exit=0
  local zmin_exit=0

  cp -R "$base_seed" "$git_work"
  cp -R "$base_seed" "$zmin_work"

  set +e
  (cd "$git_work" && "$GIT_BIN" "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files status "$git_status" "$zmin_status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_stdin_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local input="$tmpdir/${name}.stdin"
  local stdin_text="${CHECK_MAILMAP_STDIN_TEXT:-Alias Name <alias@example.com>\n}"
  local git_exit=0
  local zmin_exit=0

  cp -R "$base_seed" "$git_work"
  cp -R "$base_seed" "$zmin_work"
  printf '%b' "$stdin_text" >"$input"

  set +e
  (cd "$git_work" && "$GIT_BIN" "$@") <"$input" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (cd "$zmin_work" && "$ZMIN_BIN" "$@") <"$input" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if [ "$git_exit" != "$zmin_exit" ]; then
    echo "$name exit differs: stock=$git_exit zmin=$zmin_exit" >&2
    return 1
  fi
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

base_seed="$tmpdir/base"
make_seed_repo "$base_seed"

run_case check_mailmap_positional check-mailmap 'Alias Name <alias@example.com>'
run_case check_mailmap_multiple check-mailmap 'Alias Name <alias@example.com>' 'Other <old@example.com>'
run_case check_mailmap_mailmap_file check-mailmap --mailmap-file alt.mailmap 'Alt Alias <alt-alias@example.com>'
run_case check_mailmap_mailmap_file_equals check-mailmap --mailmap-file=alt.mailmap 'Alt Alias <alt-alias@example.com>'
blob_oid="$(cat "$base_seed/blob.oid")"
run_case check_mailmap_mailmap_blob check-mailmap --mailmap-blob "$blob_oid" 'Blob Alias <blob-alias@example.com>'
run_case check_mailmap_mailmap_blob_equals check-mailmap --mailmap-blob="$blob_oid" 'Blob Alias <blob-alias@example.com>'
run_case check_mailmap_mailmap_file_then_blob check-mailmap --mailmap-file alt.mailmap --mailmap-blob "$blob_oid" 'Blob Alias <blob-alias@example.com>'
run_case check_mailmap_mailmap_blob_then_file check-mailmap --mailmap-blob "$blob_oid" --mailmap-file alt.mailmap 'Alt Alias <alt-alias@example.com>'
run_case check_mailmap_mailmap_file_equals_then_blob_equals check-mailmap --mailmap-file=alt.mailmap --mailmap-blob="$blob_oid" 'Blob Alias <blob-alias@example.com>'
run_stdin_case check_mailmap_stdin_repeated check-mailmap --stdin --stdin
run_stdin_case check_mailmap_stdin_tripled check-mailmap --stdin --stdin --stdin
CHECK_MAILMAP_STDIN_TEXT='Alt Alias <alt-alias@example.com>\n' run_stdin_case check_mailmap_stdin_with_mailmap_file check-mailmap --stdin --mailmap-file alt.mailmap
CHECK_MAILMAP_STDIN_TEXT='Blob Alias <blob-alias@example.com>\n' run_stdin_case check_mailmap_stdin_with_mailmap_blob check-mailmap --stdin --mailmap-blob "$blob_oid"
CHECK_MAILMAP_STDIN_TEXT='Blob Alias <blob-alias@example.com>\n' run_stdin_case check_mailmap_stdin_file_then_blob check-mailmap --stdin --mailmap-file alt.mailmap --mailmap-blob "$blob_oid"
CHECK_MAILMAP_STDIN_TEXT='Alt Alias <alt-alias@example.com>\n' run_stdin_case check_mailmap_stdin_blob_then_file check-mailmap --stdin --mailmap-blob "$blob_oid" --mailmap-file alt.mailmap
run_stdin_case check_mailmap_no_then_stdin check-mailmap --no-stdin --stdin
run_stdin_case check_mailmap_stdin_stdin_no_stdin_stdin check-mailmap --stdin --stdin --no-stdin --stdin
CHECK_MAILMAP_STDIN_TEXT='Alt Alias <alt-alias@example.com>\n' run_stdin_case check_mailmap_stdin_no_stdin_stdin_file check-mailmap --stdin --no-stdin --stdin --mailmap-file alt.mailmap
run_case check_mailmap_stdin_no_stdin_with_mailmap_file_positional check-mailmap --stdin --no-stdin --mailmap-file alt.mailmap 'Alt Alias <alt-alias@example.com>'
run_case check_mailmap_stdin_no_stdin_with_mailmap_blob_positional check-mailmap --stdin --no-stdin --mailmap-blob "$blob_oid" 'Blob Alias <blob-alias@example.com>'
run_stdin_case check_mailmap_no_stdin_rejected check-mailmap --no-stdin
run_stdin_case check_mailmap_stdin_rejects_value check-mailmap --stdin=true
run_stdin_case check_mailmap_stdin_rejects_empty_value check-mailmap --stdin=
