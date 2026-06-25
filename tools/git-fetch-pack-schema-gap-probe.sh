#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-fetch-pack-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_remote() {
  local root="$1"
  mkdir "$root"
  "$GIT_BIN" -C "$root" init -q --bare remote.git
  local blob
  local tree
  local commit
  blob="$(printf 'base\n' | "$GIT_BIN" --git-dir="$root/remote.git" hash-object -w --stdin)"
  tree="$(printf '100644 blob %s\tfile.txt\n' "$blob" | "$GIT_BIN" --git-dir="$root/remote.git" mktree)"
  commit="$(printf 'base\n' | \
    GIT_AUTHOR_NAME="Oracle" \
    GIT_AUTHOR_EMAIL="oracle@example.com" \
    GIT_COMMITTER_NAME="Oracle" \
    GIT_COMMITTER_EMAIL="oracle@example.com" \
    "$GIT_BIN" --git-dir="$root/remote.git" commit-tree "$tree")"
  "$GIT_BIN" --git-dir="$root/remote.git" update-ref refs/heads/main "$commit"
  "$GIT_BIN" -C "$root/remote.git" symbolic-ref HEAD refs/heads/main
}

make_client() {
  local path="$1"
  "$GIT_BIN" init -q "$path"
}

run_gap() {
  local name="$1"
  local expected_git_exit="$2"
  local expected_zmin_exit="$3"
  local stdin_data="$4"
  shift 4
  local git_client="$tmpdir/$name.git-client"
  local zmin_client="$tmpdir/$name.zmin-client"
  local git_exit=0
  local zmin_exit=0

  make_client "$git_client"
  make_client "$zmin_client"

  set +e
  printf '%s' "$stdin_data" | "$GIT_BIN" -C "$git_client" fetch-pack "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  printf '%s' "$stdin_data" | "$ZMIN_BIN" -C "$zmin_client" fetch-pack "$@" >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  printf '%s\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
  printf 'stock stdout:\n'
  sed -n '1,6p' "$tmpdir/$name.git.out"
  printf 'zmin stdout:\n'
  sed -n '1,6p' "$tmpdir/$name.zmin.out"
  printf 'stock stderr:\n'
  sed -n '1,4p' "$tmpdir/$name.git.err"
  printf 'zmin stderr:\n'
  sed -n '1,4p' "$tmpdir/$name.zmin.err"

  test "$git_exit" = "$expected_git_exit"
  test "$zmin_exit" = "$expected_zmin_exit"
  if [ "$git_exit" = "$zmin_exit" ] \
    && cmp -s "$tmpdir/$name.git.out" "$tmpdir/$name.zmin.out" \
    && cmp -s "$tmpdir/$name.git.err" "$tmpdir/$name.zmin.err"; then
    echo "$name unexpectedly matched" >&2
    return 1
  fi
}

normalize_trace() {
  local path="$1"
  local root="$2"
  python3 - "$path" "$root" <<'PY'
import re
import sys
path, root = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8", errors="replace").read()
text = text.replace(root, "__ROOT__")
text = re.sub(r"\b[0-9a-f]{40}\b", "__OID__", text)
out = []
saw_counting = False
saw_compressing = False
for raw in re.split(r"[\r\n]+", text):
    line = raw.rstrip()
    if not line:
        continue
    if line.startswith("remote: "):
        line = line[len("remote: "):]
    if line.startswith("Enumerating objects: "):
        out.append("Enumerating objects: __COUNT__, done.")
        continue
    if line.startswith("Counting objects: "):
        if not saw_counting:
            out.append("Counting objects: __PROGRESS__")
            saw_counting = True
        continue
    if line.startswith("Compressing objects: "):
        if not saw_compressing:
            out.append("Compressing objects: __PROGRESS__")
            saw_compressing = True
        continue
    if line.startswith("Total "):
        out.append("Total __COUNT__ (delta 0), reused 0 (delta 0), pack-reused 0 (from 0)")
        continue
    out.append(line)
print("\n".join(out), end="")
PY
}

list_pack_side_effects() {
  local client="$1"
  python3 - "$client" <<'PY'
import os
import re
import sys
pack_dir = os.path.join(sys.argv[1], ".git", "objects", "pack")
if not os.path.isdir(pack_dir):
    sys.exit(0)
for name in sorted(os.listdir(pack_dir)):
    normalized = re.sub(r"pack-[0-9a-f]{40}", "pack-__PACK__", name)
    print(normalized)
PY
}

run_oracle() {
  local name="$1"
  local stdin_data="$2"
  shift 2
  local git_client="$tmpdir/$name.git-client"
  local zmin_client="$tmpdir/$name.zmin-client"
  local git_exit=0
  local zmin_exit=0

  make_client "$git_client"
  make_client "$zmin_client"

  set +e
  printf '%s' "$stdin_data" | "$GIT_BIN" -C "$git_client" fetch-pack "$@" >"$tmpdir/$name.git.out" 2>"$tmpdir/$name.git.err"
  git_exit=$?
  printf '%s' "$stdin_data" | "$ZMIN_BIN" -C "$zmin_client" fetch-pack "$@" >"$tmpdir/$name.zmin.out" 2>"$tmpdir/$name.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = "$zmin_exit"
  normalize_trace "$tmpdir/$name.git.out" "$tmpdir" >"$tmpdir/$name.git.out.norm"
  normalize_trace "$tmpdir/$name.zmin.out" "$tmpdir" >"$tmpdir/$name.zmin.out.norm"
  normalize_trace "$tmpdir/$name.git.err" "$tmpdir" >"$tmpdir/$name.git.err.norm"
  normalize_trace "$tmpdir/$name.zmin.err" "$tmpdir" >"$tmpdir/$name.zmin.err.norm"
  list_pack_side_effects "$git_client" >"$tmpdir/$name.git.pack"
  list_pack_side_effects "$zmin_client" >"$tmpdir/$name.zmin.pack"
  cmp -s "$tmpdir/$name.git.out.norm" "$tmpdir/$name.zmin.out.norm"
  cmp -s "$tmpdir/$name.git.err.norm" "$tmpdir/$name.zmin.err.norm"
  cmp -s "$tmpdir/$name.git.pack" "$tmpdir/$name.zmin.pack"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

root="$tmpdir/root"
make_remote "$root"
remote="$root/remote.git"

run_oracle fetch_pack_all "" --all "$remote"
run_oracle fetch_pack_stdin "refs/heads/main"$'\n' --stdin "$remote"
run_oracle fetch_pack_quiet "" --quiet "$remote" refs/heads/main
run_oracle fetch_pack_keep "" --keep "$remote" refs/heads/main
run_oracle fetch_pack_upload_pack "" --upload-pack=git-upload-pack "$remote" refs/heads/main
run_oracle fetch_pack_upload_pack_separate "" --upload-pack git-upload-pack "$remote" refs/heads/main
run_oracle fetch_pack_diag_url "" --diag-url "$remote"
run_oracle fetch_pack_verbose_long "" --verbose "$remote" refs/heads/main
run_oracle fetch_pack_keep_short "" -k "$remote" refs/heads/main
run_oracle fetch_pack_quiet_short "" -q "$remote" refs/heads/main
run_oracle fetch_pack_verbose_short "" -v "$remote" refs/heads/main
