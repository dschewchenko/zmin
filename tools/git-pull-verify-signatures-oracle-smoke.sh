#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/debug/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

export GIT_AUTHOR_NAME=Bench
export GIT_AUTHOR_EMAIL=bench@example.test
export GIT_AUTHOR_DATE="1700000000 +0000"
export GIT_COMMITTER_NAME=Bench
export GIT_COMMITTER_EMAIL=bench@example.test
export GIT_COMMITTER_DATE="1700000000 +0000"

tmpdir="$(mktemp -d /tmp/zmin-pull-verify-signatures.XXXXXX)"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

gpg_home=
gpg_wrapper=
gpg_key=

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

ensure_gpg_fixture() {
  if test -n "${gpg_home:-}"; then
    return
  fi
  gpg_home="$tmpdir/gnupg"
  mkdir -p "$gpg_home"
  chmod 700 "$gpg_home"
  cat >"$tmpdir/gen-key" <<'EOF'
Key-Type: RSA
Key-Length: 2048
Name-Real: Test Signer
Name-Email: signer@example.test
Expire-Date: 0
%no-protection
%commit
EOF
  GNUPGHOME="$gpg_home" gpg --batch --pinentry-mode loopback --faked-system-time 1700000000 \
    --generate-key "$tmpdir/gen-key" >/dev/null 2>/dev/null
  gpg_key="$(
    GNUPGHOME="$gpg_home" gpg --batch --with-colons --list-secret-keys signer@example.test 2>/dev/null |
      awk -F: '$1=="sec"{print $5; exit}'
  )"
  test -n "$gpg_key"
  gpg_wrapper="$tmpdir/gpg-wrap"
  cat >"$gpg_wrapper" <<EOF
#!/bin/sh
export GNUPGHOME="$gpg_home"
exec gpg --batch --pinentry-mode loopback --faked-system-time 1700000000 "\$@"
EOF
  chmod +x "$gpg_wrapper"
}

configure_repo_signing() {
  local repo="$1"
  "$GIT_BIN" -C "$repo" config user.name Bench
  "$GIT_BIN" -C "$repo" config user.email bench@example.test
  "$GIT_BIN" -C "$repo" config commit.gpgsign false
  "$GIT_BIN" -C "$repo" config user.signingkey "$gpg_key"
  "$GIT_BIN" -C "$repo" config gpg.program "$gpg_wrapper"
}

seed_pull_fixture() {
  local root="$1"
  local sign_remote_tip="$2"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"

  mkdir -p "$source"
  "$GIT_BIN" -C "$source" init -q -b main
  configure_repo_signing "$source"
  printf 'base\n' >"$source/base.txt"
  "$GIT_BIN" -C "$source" add base.txt
  "$GIT_BIN" -C "$source" commit -q -m base

  "$GIT_BIN" clone -q "$source" "$git_client"
  "$GIT_BIN" clone -q "$source" "$zmin_client"
  configure_repo_signing "$git_client"
  configure_repo_signing "$zmin_client"

  printf 'local\n' >"$git_client/local.txt"
  printf 'local\n' >"$zmin_client/local.txt"
  "$GIT_BIN" -C "$git_client" add local.txt
  "$GIT_BIN" -C "$zmin_client" add local.txt
  "$GIT_BIN" -C "$git_client" commit -q -m local
  "$GIT_BIN" -C "$zmin_client" commit -q -m local

  printf 'remote\n' >"$source/remote.txt"
  "$GIT_BIN" -C "$source" add remote.txt
  if test "$sign_remote_tip" = "signed"; then
    "$GIT_BIN" -C "$source" commit -q -S -m remote
  else
    "$GIT_BIN" -C "$source" commit -q -m remote
  fi
}

run_success_case() {
  local name="$1"
  local sign_remote_tip="$2"
  shift 2
  local root="$tmpdir/${name}.pull"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  local git_fetch_head="$tmpdir/${name}.git.fetch_head"
  local zmin_fetch_head="$tmpdir/${name}.zmin.fetch_head"

  mkdir -p "$root"
  seed_pull_fixture "$root" "$sign_remote_tip"

  "$GIT_BIN" -C "$git_client" pull --no-rebase "$@" "$source" main >"$git_out" 2>"$git_err"
  "$ZMIN_BIN" -C "$zmin_client" pull --no-rebase "$@" "$source" main >"$zmin_out" 2>"$zmin_err"

  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_client" cat-file -p HEAD >"$git_commit"
  "$GIT_BIN" -C "$zmin_client" cat-file -p HEAD >"$zmin_commit"
  compare_files commit "$git_commit" "$zmin_commit"
  cp "$git_client/.git/FETCH_HEAD" "$git_fetch_head"
  cp "$zmin_client/.git/FETCH_HEAD" "$zmin_fetch_head"
  compare_files fetch_head "$git_fetch_head" "$zmin_fetch_head"
}

run_failure_case() {
  local name="$1"
  shift
  local root="$tmpdir/${name}.pull"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_head_before="$tmpdir/${name}.git.head.before"
  local zmin_head_before="$tmpdir/${name}.zmin.head.before"
  local git_head_after="$tmpdir/${name}.git.head.after"
  local zmin_head_after="$tmpdir/${name}.zmin.head.after"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_fetch_head="$tmpdir/${name}.git.fetch_head"
  local zmin_fetch_head="$tmpdir/${name}.zmin.fetch_head"

  mkdir -p "$root"
  seed_pull_fixture "$root" unsigned

  "$GIT_BIN" -C "$git_client" rev-parse HEAD >"$git_head_before"
  "$GIT_BIN" -C "$zmin_client" rev-parse HEAD >"$zmin_head_before"

  set +e
  "$GIT_BIN" -C "$git_client" pull --no-rebase "$@" "$source" main >"$git_out" 2>"$git_err"
  local git_rc=$?
  "$ZMIN_BIN" -C "$zmin_client" pull --no-rebase "$@" "$source" main >"$zmin_out" 2>"$zmin_err"
  local zmin_rc=$?
  set -e

  test "$git_rc" -eq "$zmin_rc"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_client" rev-parse HEAD >"$git_head_after"
  "$GIT_BIN" -C "$zmin_client" rev-parse HEAD >"$zmin_head_after"
  compare_files head_before "$git_head_before" "$zmin_head_before"
  compare_files head_after "$git_head_after" "$zmin_head_after"
  compare_files head_unchanged "$git_head_before" "$git_head_after"
  compare_files head_unchanged_zmin "$zmin_head_before" "$zmin_head_after"
  "$GIT_BIN" -C "$git_client" status --porcelain=v1 --branch >"$git_status"
  "$GIT_BIN" -C "$zmin_client" status --porcelain=v1 --branch >"$zmin_status"
  compare_files status "$git_status" "$zmin_status"
  cp "$git_client/.git/FETCH_HEAD" "$git_fetch_head"
  cp "$zmin_client/.git/FETCH_HEAD" "$zmin_fetch_head"
  compare_files fetch_head "$git_fetch_head" "$zmin_fetch_head"
}

ensure_gpg_fixture
run_success_case pull_verify_signatures_good signed --verify-signatures
run_failure_case pull_verify_signatures_unsigned --verify-signatures
run_success_case pull_no_verify_signatures signed --no-verify-signatures
