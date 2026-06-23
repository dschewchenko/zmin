#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-init-gap.XXXXXX")"
tmpdir="$(cd "$tmpdir" && pwd -P)"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

mkdir -p "$tmpdir/template/hooks"
printf '# sample\n' >"$tmpdir/template/hooks/pre-commit"

normalize_text() {
  local root="$1"
  local file="$2"
  local private_root="/private${root}"
  sed \
    -e "s#$private_root/gitdir#<gitdir>#g" \
    -e "s#$private_root/zmindir#<gitdir>#g" \
    -e "s#$private_root/git#<work>#g" \
    -e "s#$private_root/zmin#<work>#g" \
    -e "s#$root/gitdir#<gitdir>#g" \
    -e "s#$root/zmindir#<gitdir>#g" \
    -e "s#$root/git#<work>#g" \
    -e "s#$root/zmin#<work>#g" \
    "$file"
}

summarize_tree() {
  local dir="$1"
  (
    cd "$dir"
    find . -maxdepth 4 \( -type d -o -type f -o -type l \) | sort | sed 's#^\./##'
  )
}

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
  shift
  local root="$tmpdir/$name"
  local git_exit=0
  local zmin_exit=0
  local git_args=()
  local zmin_args=()
  mkdir -p "$root/git" "$root/zmin"

  for arg in "$@"; do
    case "$arg" in
      __GITDIR__)
        git_args+=("$root/gitdir")
        zmin_args+=("$root/zmindir")
        ;;
      __TEMPLATE__)
        git_args+=("$tmpdir/template")
        zmin_args+=("$tmpdir/template")
        ;;
      *)
        git_args+=("$arg")
        zmin_args+=("$arg")
        ;;
    esac
  done

  set +e
  "$GIT_BIN" -C "$root/git" init "${git_args[@]}" >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$root/zmin" init "${zmin_args[@]}" >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  normalize_text "$root" "$root/git.out" >"$root/git.norm.out"
  normalize_text "$root" "$root/zmin.out" >"$root/zmin.norm.out"
  normalize_text "$root" "$root/git.err" >"$root/git.norm.err"
  normalize_text "$root" "$root/zmin.err" >"$root/zmin.norm.err"
  summarize_tree "$root/git" >"$root/git.tree"
  summarize_tree "$root/zmin" >"$root/zmin.tree"

  if [ "$git_exit" != "$zmin_exit" ]; then
    echo "$name exit differs: stock=$git_exit zmin=$zmin_exit" >&2
    return 1
  fi
  compare_files stdout "$root/git.norm.out" "$root/zmin.norm.out"
  compare_files stderr "$root/git.norm.err" "$root/zmin.norm.err"
  compare_files tree "$root/git.tree" "$root/zmin.tree"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_case init_bare --bare repo.git
run_case init_initial_branch --initial-branch main repo
run_case init_separate_git_dir --separate-git-dir __GITDIR__ repo
run_case init_shared_group --shared=group repo
run_case init_template --template=__TEMPLATE__ repo
