#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

export GIT_AUTHOR_NAME=Oracle
export GIT_AUTHOR_EMAIL=oracle@example.com
export GIT_AUTHOR_DATE="1700000000 +0000"
export GIT_COMMITTER_NAME=Oracle
export GIT_COMMITTER_EMAIL=oracle@example.com
export GIT_COMMITTER_DATE="1700000000 +0000"

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-multi-pack-index-oracle.XXXXXX")"
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

seed_two_pack_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name Oracle
  "$GIT_BIN" -C "$repo" config user.email oracle@example.com

  printf 'one\n' >"$repo/one.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m one
  "$GIT_BIN" -C "$repo" rev-list --objects --no-object-names HEAD |
    "$GIT_BIN" -C "$repo" pack-objects .git/objects/pack/pack >/dev/null

  export GIT_AUTHOR_DATE="1700000001 +0000"
  export GIT_COMMITTER_DATE="1700000001 +0000"
  printf 'two\n' >"$repo/two.txt"
  "$GIT_BIN" -C "$repo" add -A
  "$GIT_BIN" -C "$repo" commit -q -m two
  "$GIT_BIN" -C "$repo" rev-list --objects --no-object-names --all |
    "$GIT_BIN" -C "$repo" pack-objects .git/objects/pack/pack >/dev/null
  export GIT_AUTHOR_DATE="1700000000 +0000"
  export GIT_COMMITTER_DATE="1700000000 +0000"
}

pack_index_names() {
  local repo="$1"
  find "$repo/.git/objects/pack" -maxdepth 1 -name '*.idx' -exec basename {} \; | sort
}

resolve_multi_pack_index_arg() {
  local repo="$1"
  local arg="$2"
  local first_idx second_idx
  first_idx="$(pack_index_names "$repo" | sed -n '1p')"
  second_idx="$(pack_index_names "$repo" | sed -n '2p')"
  arg="${arg//__FIRST_PACK_IDX__/$first_idx}"
  arg="${arg//__SECOND_PACK_PACK__/${second_idx%.idx}.pack}"
  printf '%s' "$arg"
}

compare_midx() {
  local git_work="$1"
  local zmin_work="$2"
  cmp -s "$git_work/.git/objects/pack/multi-pack-index" \
    "$zmin_work/.git/objects/pack/multi-pack-index"
}

run_write_exact() {
  local name="$1"
  local stdin_text="${2:-}"
  local compare_bytes="${3:-yes}"
  shift 3
  local git_work="$tmpdir/${name}.git.work"
  local zmin_work="$tmpdir/${name}.zmin.work"
  local git_exit=0
  local zmin_exit=0

  seed_two_pack_repo "$git_work"
  cp -R "$git_work" "$zmin_work"
  local resolved_stdin_text
  resolved_stdin_text="$(resolve_multi_pack_index_arg "$git_work" "$stdin_text")"
  local resolved_args=()
  local arg
  local pre_args=()
  local write_args=()
  for arg in "$@"; do
    resolved_args+=("$(resolve_multi_pack_index_arg "$git_work" "$arg")")
  done
  for arg in "${resolved_args[@]}"; do
    if [[ "$arg" == --object-dir=* ]]; then
      pre_args+=("$arg")
    else
      write_args+=("$arg")
    fi
  done
  local git_cmd=("$GIT_BIN" -C "$git_work" multi-pack-index)
  local zmin_cmd=("$ZMIN_BIN" -C "$zmin_work" multi-pack-index)
  if ((${#pre_args[@]})); then
    git_cmd+=("${pre_args[@]}")
    zmin_cmd+=("${pre_args[@]}")
  fi
  git_cmd+=(write)
  zmin_cmd+=(write)
  if ((${#write_args[@]})); then
    git_cmd+=("${write_args[@]}")
    zmin_cmd+=("${write_args[@]}")
  fi

  set +e
  if [[ -n "$resolved_stdin_text" ]]; then
    printf '%b' "$resolved_stdin_text" |
      "${git_cmd[@]}" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
    git_exit=$?
    printf '%b' "$resolved_stdin_text" |
      "${zmin_cmd[@]}" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
    zmin_exit=$?
  else
    "${git_cmd[@]}" >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
    git_exit=$?
    "${zmin_cmd[@]}" >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
    zmin_exit=$?
  fi
  set -e

  test "$git_exit" = "$zmin_exit"
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  if test -f "$git_work/.git/objects/pack/multi-pack-index"; then
    test -f "$zmin_work/.git/objects/pack/multi-pack-index"
    if [[ "$compare_bytes" == yes ]]; then
      compare_midx "$git_work" "$zmin_work"
    fi
    "$GIT_BIN" -C "$git_work" multi-pack-index verify >"$tmpdir/${name}.git.verify.out" 2>"$tmpdir/${name}.git.verify.err"
    "$GIT_BIN" -C "$zmin_work" multi-pack-index verify >"$tmpdir/${name}.zmin.verify.out" 2>"$tmpdir/${name}.zmin.verify.err"
    compare_files verify-stdout "$tmpdir/${name}.git.verify.out" "$tmpdir/${name}.zmin.verify.out"
    compare_files verify-stderr "$tmpdir/${name}.git.verify.err" "$tmpdir/${name}.zmin.verify.err"
  else
    test ! -f "$zmin_work/.git/objects/pack/multi-pack-index"
  fi
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_write_exact multi_pack_index_object_dir_write "" no --object-dir=.git/objects
run_write_exact multi_pack_index_no_bitmap_write "" yes --no-bitmap
run_write_exact multi_pack_index_preferred_pack "" yes --preferred-pack=__SECOND_PACK_PACK__
