#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-init-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

mkdir -p "$tmpdir/template/hooks"
printf '# sample\n' >"$tmpdir/template/hooks/pre-commit"

normalize_text() {
  local root="$1"
  local file="$2"
  sed \
    -e "s#$root/git#<work>#g" \
    -e "s#$root/zmin#<work>#g" \
    -e "s#$root/gitdir#<gitdir>#g" \
    -e "s#$root/zmindir#<gitdir>#g" \
    "$file"
}

summarize_tree() {
  local dir="$1"
  (
    cd "$dir"
    find . -maxdepth 4 \( -type d -o -type f -o -type l \) | sort | sed 's#^\./##'
  )
}

run_gap() {
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
  summarize_tree "$root/git" >"$root/git.tree"
  summarize_tree "$root/zmin" >"$root/zmin.tree"

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  printf 'stock stdout:\n'
  sed -n '1,4p' "$root/git.norm.out"
  printf 'zmin stdout:\n'
  sed -n '1,4p' "$root/zmin.norm.out"
  printf 'stock tree:\n'
  sed -n '1,20p' "$root/git.tree"
  printf 'zmin tree:\n'
  sed -n '1,20p' "$root/zmin.tree"

  test "$git_exit" = "$zmin_exit"
  if cmp -s "$root/git.norm.out" "$root/zmin.norm.out" \
    && cmp -s "$root/git.err" "$root/zmin.err" \
    && cmp -s "$root/git.tree" "$root/zmin.tree"; then
    echo "$name unexpectedly matched" >&2
    return 1
  fi
}

run_gap init_bare --bare repo.git
run_gap init_separate_git_dir --separate-git-dir __GITDIR__ repo
run_gap init_shared_group --shared=group repo
run_gap init_template --template=__TEMPLATE__ repo
