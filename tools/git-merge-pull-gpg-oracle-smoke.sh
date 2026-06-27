#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-all}"
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

tmpdir="$(mktemp -d /tmp/zmin-merge-pull-gpg-oracle.XXXXXX)"
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

configure_repo_signing() {
  local repo="$1"
  local gpgsign="${2:-false}"
  "$GIT_BIN" -C "$repo" config user.name Bench
  "$GIT_BIN" -C "$repo" config user.email bench@example.test
  "$GIT_BIN" -C "$repo" config commit.gpgsign "$gpgsign"
  "$GIT_BIN" -C "$repo" config user.signingkey "$gpg_key"
  "$GIT_BIN" -C "$repo" config gpg.program "$gpg_wrapper"
}

seed_merge_repo() {
  local repo="$1"
  local gpgsign="${2:-false}"
  mkdir -p "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  configure_repo_signing "$repo" "$gpgsign"
  printf 'base\n' >"$repo/base.txt"
  "$GIT_BIN" -C "$repo" add base.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" switch -q -c feature
  printf 'feature\n' >"$repo/feature.txt"
  "$GIT_BIN" -C "$repo" add feature.txt
  "$GIT_BIN" -C "$repo" commit -q -m feature
  "$GIT_BIN" -C "$repo" switch -q main
  printf 'main\n' >"$repo/main.txt"
  "$GIT_BIN" -C "$repo" add main.txt
  "$GIT_BIN" -C "$repo" commit -q -m main
}

run_merge_case() {
  local name="$1"
  local repo_gpgsign="$2"
  shift 2
  local git_repo="$tmpdir/${name}.git.repo"
  local zmin_repo="$tmpdir/${name}.zmin.repo"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"

  seed_merge_repo "$git_repo" "$repo_gpgsign"
  seed_merge_repo "$zmin_repo" "$repo_gpgsign"

  "$GIT_BIN" -C "$git_repo" merge "$@" >"$git_out" 2>"$git_err"
  "$ZMIN_BIN" -C "$zmin_repo" merge "$@" >"$zmin_out" 2>"$zmin_err"

  compare_files merge_stdout "$git_out" "$zmin_out"
  compare_files merge_stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_repo" cat-file -p HEAD >"$git_commit"
  "$GIT_BIN" -C "$zmin_repo" cat-file -p HEAD >"$zmin_commit"
  compare_files merge_commit "$git_commit" "$zmin_commit"
  printf '%s\tok\n' "$name"
}

seed_pull_fixture() {
  local root="$1"
  local repo_gpgsign="$2"
  local source="$root/source"
  local git_client="$root/git-client"
  local zmin_client="$root/zmin-client"

  mkdir -p "$source"
  "$GIT_BIN" -C "$source" init -q -b main
  configure_repo_signing "$source" false
  printf 'base\n' >"$source/base.txt"
  "$GIT_BIN" -C "$source" add base.txt
  "$GIT_BIN" -C "$source" commit -q -m base

  "$GIT_BIN" clone -q "$source" "$git_client"
  "$GIT_BIN" clone -q "$source" "$zmin_client"
  configure_repo_signing "$git_client" "$repo_gpgsign"
  configure_repo_signing "$zmin_client" "$repo_gpgsign"

  printf 'local\n' >"$git_client/local.txt"
  printf 'local\n' >"$zmin_client/local.txt"
  "$GIT_BIN" -C "$git_client" add local.txt
  "$GIT_BIN" -C "$zmin_client" add local.txt
  "$GIT_BIN" -C "$git_client" commit -q -m local
  "$GIT_BIN" -C "$zmin_client" commit -q -m local

  printf 'remote\n' >"$source/remote.txt"
  "$GIT_BIN" -C "$source" add remote.txt
  "$GIT_BIN" -C "$source" commit -q -m remote
}

run_pull_case() {
  local name="$1"
  local repo_gpgsign="$2"
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

  mkdir -p "$root"
  seed_pull_fixture "$root" "$repo_gpgsign"

  "$GIT_BIN" -C "$git_client" pull "$@" "$source" main >"$git_out" 2>"$git_err"
  "$ZMIN_BIN" -C "$zmin_client" pull "$@" "$source" main >"$zmin_out" 2>"$zmin_err"

  compare_files pull_stdout "$git_out" "$zmin_out"
  compare_files pull_stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_client" cat-file -p HEAD >"$git_commit"
  "$GIT_BIN" -C "$zmin_client" cat-file -p HEAD >"$zmin_commit"
  compare_files pull_commit "$git_commit" "$zmin_commit"
  printf '%s\tok\n' "$name"
}

run_merge_suite() {
  run_merge_case merge_gpg_sign_long false --gpg-sign feature
  run_merge_case merge_gpg_sign_short false -S feature
  run_merge_case merge_no_gpg_sign true --no-gpg-sign feature
}

run_pull_suite() {
  run_pull_case pull_gpg_sign_long false --no-rebase --gpg-sign
  run_pull_case pull_gpg_sign_short false --no-rebase -S
  run_pull_case pull_no_gpg_sign true --no-rebase --no-gpg-sign
}

ensure_gpg_fixture
case "$MODE" in
  merge) run_merge_suite ;;
  pull) run_pull_suite ;;
  all)
    run_merge_suite
    run_pull_suite
    ;;
  *)
    echo "unknown mode: $MODE" >&2
    exit 2
    ;;
esac
