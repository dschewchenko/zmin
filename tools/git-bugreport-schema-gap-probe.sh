#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
export GIT_EDITOR="${GIT_EDITOR:-:}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-bugreport-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

normalize_err() {
  local outdir="$1"
  local file="$2"
  perl -0pi -e "s#'$outdir/[^']+'#'<OUTFILE>'#g" "$file"
}

prepare_repo() {
  local repo="$1"
  "$GIT_BIN" -C "$repo" init -q
  "$GIT_BIN" -C "$repo" config user.name "Tester"
  "$GIT_BIN" -C "$repo" config user.email "tester@example.com"
  printf 'content\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -qm initial
}

run_gap() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_outdir="$tmpdir/${name}.git.outdir"
  local zmin_outdir="$tmpdir/${name}.zmin.outdir"
  local git_exit=0
  local zmin_exit=0
  mkdir "$git_work" "$zmin_work" "$git_outdir" "$zmin_outdir"
  prepare_repo "$git_work"
  prepare_repo "$zmin_work"

  set +e
  "$GIT_BIN" -C "$git_work" bugreport -o "$git_outdir" "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_work" bugreport -o "$zmin_outdir" "$@" >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  find "$git_outdir" -maxdepth 3 -type f | sed "s#$git_outdir/##" | sort >"$tmpdir/$name.git.files"
  find "$zmin_outdir" -maxdepth 3 -type f | sed "s#$zmin_outdir/##" | sort >"$tmpdir/$name.zmin.files"
  normalize_err "$git_outdir" "$tmpdir/$name.git.err"
  normalize_err "$zmin_outdir" "$tmpdir/$name.zmin.err"

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  printf 'stock stdout:\n'
  sed -n '1,16p' "$tmpdir/$name.git.out"
  printf 'zmin stdout:\n'
  sed -n '1,16p' "$tmpdir/$name.zmin.out"
  printf 'stock files:\n'
  cat "$tmpdir/$name.git.files"
  printf 'zmin files:\n'
  cat "$tmpdir/$name.zmin.files"

  test "$git_exit" = "$zmin_exit"
  cmp -s "$tmpdir/$name.git.files" "$tmpdir/$name.zmin.files"
  if cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out" \
    && cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"; then
    echo "$name unexpectedly matched" >&2
    return 1
  fi
}

run_gap bugreport_diagnose_stats --suffix diag --diagnose=stats
