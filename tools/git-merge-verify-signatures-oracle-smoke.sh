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

tmpdir="$(mktemp -d /tmp/zmin-merge-verify-signatures.XXXXXX)"
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
    GNUPGHOME="$gpg_home" gpg --batch --with-colons --list-secret-keys signer@example.test |
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

seed_repo() {
  local repo="$1"
  local sign_feature="$2"
  mkdir -p "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Bench
  "$GIT_BIN" -C "$repo" config user.email bench@example.test
  "$GIT_BIN" -C "$repo" config commit.gpgsign false
  "$GIT_BIN" -C "$repo" config user.signingkey "$gpg_key"
  "$GIT_BIN" -C "$repo" config gpg.program "$gpg_wrapper"
  printf 'base\n' >"$repo/base.txt"
  "$GIT_BIN" -C "$repo" add base.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" switch -q -c feature
  printf 'feature\n' >"$repo/feature.txt"
  "$GIT_BIN" -C "$repo" add feature.txt
  if test "$sign_feature" = "signed"; then
    "$GIT_BIN" -C "$repo" commit -q -S -m feature
  else
    "$GIT_BIN" -C "$repo" commit -q -m feature
  fi
  "$GIT_BIN" -C "$repo" switch -q main
  printf 'main\n' >"$repo/main.txt"
  "$GIT_BIN" -C "$repo" add main.txt
  "$GIT_BIN" -C "$repo" commit -q -m main
}

run_success_case() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git.repo"
  local zmin_repo="$tmpdir/${name}.zmin.repo"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"

  seed_repo "$git_repo" signed
  seed_repo "$zmin_repo" signed

  "$GIT_BIN" -C "$git_repo" merge "$@" feature >"$git_out" 2>"$git_err"
  "$ZMIN_BIN" -C "$zmin_repo" merge "$@" feature >"$zmin_out" 2>"$zmin_err"

  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_repo" cat-file -p HEAD >"$git_commit"
  "$GIT_BIN" -C "$zmin_repo" cat-file -p HEAD >"$zmin_commit"
  compare_files commit "$git_commit" "$zmin_commit"
}

run_failure_case() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git.repo"
  local zmin_repo="$tmpdir/${name}.zmin.repo"
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

  seed_repo "$git_repo" unsigned
  seed_repo "$zmin_repo" unsigned

  "$GIT_BIN" -C "$git_repo" rev-parse HEAD >"$git_head_before"
  "$GIT_BIN" -C "$zmin_repo" rev-parse HEAD >"$zmin_head_before"

  set +e
  "$GIT_BIN" -C "$git_repo" merge "$@" feature >"$git_out" 2>"$git_err"
  local git_rc=$?
  "$ZMIN_BIN" -C "$zmin_repo" merge "$@" feature >"$zmin_out" 2>"$zmin_err"
  local zmin_rc=$?
  set -e

  test "$git_rc" -eq "$zmin_rc"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_repo" rev-parse HEAD >"$git_head_after"
  "$GIT_BIN" -C "$zmin_repo" rev-parse HEAD >"$zmin_head_after"
  compare_files head_before "$git_head_before" "$zmin_head_before"
  compare_files head_after "$git_head_after" "$zmin_head_after"
  compare_files head_unchanged "$git_head_before" "$git_head_after"
  compare_files head_unchanged_zmin "$zmin_head_before" "$zmin_head_after"
  "$GIT_BIN" -C "$git_repo" status --porcelain=v1 --branch >"$git_status"
  "$GIT_BIN" -C "$zmin_repo" status --porcelain=v1 --branch >"$zmin_status"
  compare_files status "$git_status" "$zmin_status"
}

ensure_gpg_fixture
run_success_case merge_verify_signatures_good --verify-signatures
run_failure_case merge_verify_signatures_unsigned --verify-signatures
run_success_case merge_no_verify_signatures --no-verify-signatures
