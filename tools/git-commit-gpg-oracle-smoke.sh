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

tmpdir="$(mktemp -d /tmp/zmin-commit-gpg-oracle.XXXXXX)"
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
  local bin="$1"
  local repo="$2"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name Bench
  "$GIT_BIN" -C "$repo" config user.email bench@example.test
  "$GIT_BIN" -C "$repo" config commit.gpgsign false
  "$GIT_BIN" -C "$repo" config user.signingkey "$gpg_key"
  "$GIT_BIN" -C "$repo" config gpg.program "$gpg_wrapper"
  printf 'base\n' >"$repo/a.txt"
  "$bin" -C "$repo" add a.txt
  "$bin" -C "$repo" commit -m base >/dev/null
}

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"

  seed_repo "$GIT_BIN" "$git_work"
  seed_repo "$ZMIN_BIN" "$zmin_work"
  printf '%s\n' "$name" >"$git_work/a.txt"
  printf '%s\n' "$name" >"$zmin_work/a.txt"
  "$GIT_BIN" -C "$git_work" add a.txt
  "$ZMIN_BIN" -C "$zmin_work" add a.txt

  "$GIT_BIN" -C "$git_work" commit "$@" >"$git_out" 2>"$git_err"
  "$ZMIN_BIN" -C "$zmin_work" commit "$@" >"$zmin_out" 2>"$zmin_err"

  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" cat-file -p HEAD >"$git_commit"
  "$GIT_BIN" -C "$zmin_work" cat-file -p HEAD >"$zmin_commit"
  compare_files commit_object "$git_commit" "$zmin_commit"
  printf '%s\tok\n' "$name"
}

run_no_gpg_sign_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"

  seed_repo "$GIT_BIN" "$git_work"
  seed_repo "$ZMIN_BIN" "$zmin_work"
  "$GIT_BIN" -C "$git_work" config commit.gpgsign true
  "$GIT_BIN" -C "$zmin_work" config commit.gpgsign true
  printf 'unsigned\n' >"$git_work/a.txt"
  printf 'unsigned\n' >"$zmin_work/a.txt"
  "$GIT_BIN" -C "$git_work" add a.txt
  "$ZMIN_BIN" -C "$zmin_work" add a.txt

  "$GIT_BIN" -C "$git_work" commit "$@" >"$git_out" 2>"$git_err"
  "$ZMIN_BIN" -C "$zmin_work" commit "$@" >"$zmin_out" 2>"$zmin_err"

  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" cat-file -p HEAD >"$git_commit"
  "$GIT_BIN" -C "$zmin_work" cat-file -p HEAD >"$zmin_commit"
  compare_files commit_object "$git_commit" "$zmin_commit"
  if grep -q '^gpgsig ' "$git_commit" || grep -q '^gpgsig ' "$zmin_commit"; then
    echo "no-gpg-sign produced a signed commit" >&2
    return 1
  fi
  printf '%s\tok\n' "$name"
}

ensure_gpg_fixture
run_case commit_gpg_sign_long --gpg-sign -m signed
run_case commit_gpg_sign_short -S -m signed-short
run_case commit_gpg_sign_long_repeat --gpg-sign --gpg-sign -m signed-repeat
run_case commit_gpg_sign_short_repeat -S -S -m signed-short-repeat
run_case commit_gpg_sign_mixed_repeat --gpg-sign -S -m signed-mixed
run_no_gpg_sign_case commit_no_gpg_sign --no-gpg-sign -m unsigned
run_no_gpg_sign_case commit_no_gpg_sign_repeat --no-gpg-sign --no-gpg-sign -m unsigned-repeat
run_no_gpg_sign_case commit_gpg_then_no_gpg_sign --gpg-sign --no-gpg-sign -m unsigned-override
