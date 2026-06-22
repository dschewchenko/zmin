#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

export GIT_AUTHOR_NAME="${GIT_AUTHOR_NAME:-Oracle}"
export GIT_AUTHOR_EMAIL="${GIT_AUTHOR_EMAIL:-oracle@example.com}"
export GIT_AUTHOR_DATE="${GIT_AUTHOR_DATE:-1700000000 +0000}"
export GIT_COMMITTER_NAME="${GIT_COMMITTER_NAME:-Oracle}"
export GIT_COMMITTER_EMAIL="${GIT_COMMITTER_EMAIL:-oracle@example.com}"
export GIT_COMMITTER_DATE="${GIT_COMMITTER_DATE:-1700000000 +0000}"

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-commit-short-options-oracle.XXXXXX")"
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

compare_optional_files() {
  local label="$1"
  local left="$2"
  local right="$3"
  local left_exists=0
  local right_exists=0
  [ -e "$left" ] && left_exists=1
  [ -e "$right" ] && right_exists=1
  if [ "$left_exists" != "$right_exists" ]; then
    echo "$label presence differs" >&2
    ls -l "$left" "$right" >&2 || true
    return 1
  fi
  if [ "$left_exists" = 1 ]; then
    compare_files "$label" "$left" "$right"
  fi
}

seed_common() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  "$GIT_BIN" -C "$repo" config commit.gpgsign false
  printf 'base\n' >"$repo/a.txt"
  printf 'base\n' >"$repo/b.txt"
  "$GIT_BIN" -C "$repo" add a.txt b.txt
  "$GIT_BIN" -C "$repo" commit -qm base
}

prepare_basic_repo() {
  local repo="$1"
  seed_common "$repo"
  printf 'next\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  printf 'template subject\n\nbody\n' >"$repo/template.txt"
  cat >"$repo/.git/editor.sh" <<'EOS'
#!/usr/bin/env bash
printf 'edited subject\n\nedited body\n' >"$1"
EOS
  chmod +x "$repo/.git/editor.sh"
}

prepare_only_repo() {
  local repo="$1"
  seed_common "$repo"
  printf 'a2\n' >"$repo/a.txt"
  printf 'b2\n' >"$repo/b.txt"
  "$GIT_BIN" -C "$repo" add a.txt b.txt
}

prepare_hook_repo() {
  local repo="$1"
  prepare_basic_repo "$repo"
  mkdir -p "$repo/.git/hooks"
  cat >"$repo/.git/hooks/pre-commit" <<'EOS'
#!/usr/bin/env bash
echo pre-commit-ran >>.git/hook.log
echo pre-commit fail >&2
exit 42
EOS
  chmod +x "$repo/.git/hooks/pre-commit"
}

run_case() {
  local name="$1"
  local prepare="$2"
  shift 2
  local seed_work="$tmpdir/${name}.seed"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_out="$tmpdir/${name}.git.out"
  local git_err="$tmpdir/${name}.git.err"
  local zmin_out="$tmpdir/${name}.zmin.out"
  local zmin_err="$tmpdir/${name}.zmin.err"
  local git_status="$tmpdir/${name}.git.status"
  local zmin_status="$tmpdir/${name}.zmin.status"
  local git_head="$tmpdir/${name}.git.head"
  local zmin_head="$tmpdir/${name}.zmin.head"
  local git_commit="$tmpdir/${name}.git.commit"
  local zmin_commit="$tmpdir/${name}.zmin.commit"
  local git_exit=0
  local zmin_exit=0

  "$prepare" "$seed_work"
  cp -R "$seed_work" "$git_work"
  cp -R "$seed_work" "$zmin_work"

  set +e
  (cd "$git_work" && GIT_EDITOR=.git/editor.sh "$GIT_BIN" -c commit.gpgsign=false commit "$@") >"$git_out" 2>"$git_err"
  git_exit=$?
  (cd "$zmin_work" && GIT_EDITOR=.git/editor.sh "$ZMIN_BIN" -c commit.gpgsign=false commit "$@") >"$zmin_out" 2>"$zmin_err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$git_out" "$zmin_out"
  compare_files stderr "$git_err" "$zmin_err"
  "$GIT_BIN" -C "$git_work" status --short >"$git_status"
  "$GIT_BIN" -C "$zmin_work" status --short >"$zmin_status"
  compare_files status "$git_status" "$zmin_status"
  "$GIT_BIN" -C "$git_work" rev-parse HEAD >"$git_head"
  "$GIT_BIN" -C "$zmin_work" rev-parse HEAD >"$zmin_head"
  compare_files head "$git_head" "$zmin_head"
  "$GIT_BIN" -C "$git_work" cat-file -p HEAD >"$git_commit"
  "$GIT_BIN" -C "$zmin_work" cat-file -p HEAD >"$zmin_commit"
  compare_files commit "$git_commit" "$zmin_commit"
  compare_optional_files hook-log "$git_work/.git/hook.log" "$zmin_work/.git/hook.log"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case commit_quiet_short prepare_basic_repo -q -m quiet
run_case commit_signoff_short prepare_basic_repo -s -m subject
run_case commit_only_short prepare_only_repo -o -m only -- a.txt
run_case commit_no_verify_short prepare_hook_repo -n -m skip
run_case commit_template_short prepare_basic_repo -t template.txt
run_case commit_verbose_long prepare_basic_repo --verbose --no-status
