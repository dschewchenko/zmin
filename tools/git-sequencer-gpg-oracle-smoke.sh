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

tmpdir="$(mktemp -d /tmp/zmin-sequencer-gpg-oracle.XXXXXX)"
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
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name Bench
  "$GIT_BIN" -C "$repo" config user.email bench@example.test
  "$GIT_BIN" -C "$repo" config commit.gpgsign false
  "$GIT_BIN" -C "$repo" config user.signingkey "$gpg_key"
  "$GIT_BIN" -C "$repo" config gpg.program "$gpg_wrapper"
  printf 'base\n' >"$repo/base.txt"
  "$GIT_BIN" -C "$repo" add base.txt
  "$GIT_BIN" -C "$repo" commit -q -m base
  "$GIT_BIN" -C "$repo" checkout -q -b feature
  printf 'feature\n' >"$repo/feature.txt"
  "$GIT_BIN" -C "$repo" add feature.txt
  "$GIT_BIN" -C "$repo" commit -q -m feature
  local base
  base="$("$GIT_BIN" -C "$repo" rev-parse HEAD^)"
  "$GIT_BIN" -C "$repo" checkout -q -B topic "$base"
}

run_cherry_pick_case() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  seed_repo "$git_repo"
  cp -R "$git_repo" "$zmin_repo"
  "$GIT_BIN" -C "$zmin_repo" config user.name Bench
  "$GIT_BIN" -C "$zmin_repo" config user.email bench@example.test
  "$GIT_BIN" -C "$zmin_repo" config user.signingkey "$gpg_key"
  "$GIT_BIN" -C "$zmin_repo" config gpg.program "$gpg_wrapper"
  local feature
  feature="$("$GIT_BIN" -C "$git_repo" rev-parse feature)"
  "$GIT_BIN" -C "$git_repo" cherry-pick "$@" "$feature" >"$git_out" 2>"$git_err"
  "$ZMIN_BIN" -C "$zmin_repo" cherry-pick "$@" "$feature" >"$zmin_out" 2>"$zmin_err"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_repo" cat-file -p HEAD >"$git_commit"
  "$GIT_BIN" -C "$zmin_repo" cat-file -p HEAD >"$zmin_commit"
  compare_files commit_object "$git_commit" "$zmin_commit"
  printf '%s\tok\n' "$name"
}

run_revert_case() {
  local name="$1"
  shift
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  seed_repo "$git_repo"
  cp -R "$git_repo" "$zmin_repo"
  "$GIT_BIN" -C "$zmin_repo" config user.name Bench
  "$GIT_BIN" -C "$zmin_repo" config user.email bench@example.test
  "$GIT_BIN" -C "$zmin_repo" config user.signingkey "$gpg_key"
  "$GIT_BIN" -C "$zmin_repo" config gpg.program "$gpg_wrapper"
  "$GIT_BIN" -C "$git_repo" checkout -q feature
  "$GIT_BIN" -C "$zmin_repo" checkout -q feature
  "$GIT_BIN" -C "$git_repo" revert --no-edit "$@" HEAD >"$git_out" 2>"$git_err"
  "$ZMIN_BIN" -C "$zmin_repo" revert --no-edit "$@" HEAD >"$zmin_out" 2>"$zmin_err"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_repo" cat-file -p HEAD >"$git_commit"
  "$GIT_BIN" -C "$zmin_repo" cat-file -p HEAD >"$zmin_commit"
  compare_files commit_object "$git_commit" "$zmin_commit"
  printf '%s\tok\n' "$name"
}

run_unsigned_case() {
  local name="$1"
  local mode="$2"
  local git_repo="$tmpdir/${name}.git"
  local zmin_repo="$tmpdir/${name}.zmin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  seed_repo "$git_repo"
  cp -R "$git_repo" "$zmin_repo"
  for repo in "$git_repo" "$zmin_repo"; do
    "$GIT_BIN" -C "$repo" config user.name Bench
    "$GIT_BIN" -C "$repo" config user.email bench@example.test
    "$GIT_BIN" -C "$repo" config user.signingkey "$gpg_key"
    "$GIT_BIN" -C "$repo" config gpg.program "$gpg_wrapper"
    "$GIT_BIN" -C "$repo" config commit.gpgsign true
  done
  if test "$mode" = cherry-pick; then
    local feature
    feature="$("$GIT_BIN" -C "$git_repo" rev-parse feature)"
    "$GIT_BIN" -C "$git_repo" cherry-pick --no-gpg-sign "$feature" >"$git_out" 2>"$git_err"
    "$ZMIN_BIN" -C "$zmin_repo" cherry-pick --no-gpg-sign "$feature" >"$zmin_out" 2>"$zmin_err"
  else
    "$GIT_BIN" -C "$git_repo" checkout -q feature
    "$GIT_BIN" -C "$zmin_repo" checkout -q feature
    "$GIT_BIN" -C "$git_repo" revert --no-edit --no-gpg-sign HEAD >"$git_out" 2>"$git_err"
    "$ZMIN_BIN" -C "$zmin_repo" revert --no-edit --no-gpg-sign HEAD >"$zmin_out" 2>"$zmin_err"
  fi
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_repo" cat-file -p HEAD >"$git_commit"
  "$GIT_BIN" -C "$zmin_repo" cat-file -p HEAD >"$zmin_commit"
  compare_files commit_object "$git_commit" "$zmin_commit"
  if grep -q '^gpgsig ' "$git_commit" || grep -q '^gpgsig ' "$zmin_commit"; then
    echo "$name produced a signed commit" >&2
    return 1
  fi
  printf '%s\tok\n' "$name"
}

ensure_gpg_fixture
run_cherry_pick_case cherry_pick_gpg_sign_long --gpg-sign
run_cherry_pick_case cherry_pick_gpg_sign_short -S
run_unsigned_case cherry_pick_no_gpg_sign cherry-pick
run_revert_case revert_gpg_sign_long --gpg-sign
run_revert_case revert_gpg_sign_short -S
run_unsigned_case revert_no_gpg_sign revert
