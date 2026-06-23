#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-replace-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf 'one\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -qm "one"
  printf 'two\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" commit -am "two" -q
}

list_replace_refs() {
  local repo="$1"
  "$GIT_BIN" -C "$repo" for-each-ref --format='%(refname) %(objectname)' refs/replace | sort
}

run_edit_gap() {
  local name="$1"
  local option="$2"
  local root="$tmpdir/$name"
  local git_repo="$root/git"
  local zmin_repo="$root/zmin"
  local git_exit=0
  local zmin_exit=0
  mkdir "$root"
  make_repo "$git_repo"
  cp -R "$git_repo" "$zmin_repo"
  object="$("$GIT_BIN" -C "$git_repo" rev-parse HEAD)"
  cat >"$root/editor.sh" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$root/editor.sh"

  set +e
  GIT_EDITOR="$root/editor.sh" "$GIT_BIN" -C "$git_repo" replace "$option" --edit "$object" >"$root/git.out" 2>"$root/git.err"
  git_exit=$?
  GIT_EDITOR="$root/editor.sh" "$ZMIN_BIN" -C "$zmin_repo" replace "$option" --edit "$object" >"$root/zmin.out" 2>"$root/zmin.err"
  zmin_exit=$?
  set -e

  list_replace_refs "$git_repo" >"$root/git.refs"
  list_replace_refs "$zmin_repo" >"$root/zmin.refs"
  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  printf 'stock stderr:\n'
  sed -n '1,4p' "$root/git.err"
  printf 'zmin stderr:\n'
  sed -n '1,4p' "$root/zmin.err"
  test "$git_exit" = 255
  test "$zmin_exit" = 0
}

run_edit_gap replace_raw_edit_noop --raw
run_edit_gap replace_no_raw_edit_noop --no-raw
