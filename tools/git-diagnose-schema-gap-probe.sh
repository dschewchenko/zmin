#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-diagnose-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_repo() {
  local repo="$1"
  "$GIT_BIN" init -q -b main "$repo"
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.com"
  printf 'content\n' >"$repo/a.txt"
  "$GIT_BIN" -C "$repo" add a.txt
  "$GIT_BIN" -C "$repo" commit -qm "initial"
}

list_zip() {
  local path="$1"
  python3 - "$path" <<'PY'
import sys
import zipfile
with zipfile.ZipFile(sys.argv[1]) as archive:
    for name in sorted(archive.namelist()):
        data = archive.read(name).decode("utf-8", "replace")
        print(f"### {name}")
        print(data, end="" if data.endswith("\n") else "\n")
PY
}

normalize_text() {
  local path="$1"
  local root="$2"
  python3 - "$path" "$root" <<'PY'
import re
import sys
path, root = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8", errors="replace").read()
text = text.replace(root, "__ROOT__")
text = text.replace("git-repo", "__REPO__").replace("zmin-repo", "__REPO__")
text = text.replace("git-out", "__OUT__").replace("zmin-out", "__OUT__")
text = re.sub(r"Available space on '([^']+)': [0-9.]+ GiB", r"Available space on '\1': __SPACE__ GiB", text)
print(text, end="")
PY
}

run_oracle() {
  local name="$1"
  local expected_file="$2"
  shift 2
  local root="$tmpdir/$name"
  local git_repo="$root/git-repo"
  local zmin_repo="$root/zmin-repo"
  local git_out="$root/git-out"
  local zmin_out="$root/zmin-out"
  local git_exit=0
  local zmin_exit=0

  mkdir -p "$root"
  make_repo "$git_repo"
  cp -R "$git_repo" "$zmin_repo"
  mkdir "$git_out" "$zmin_out"

  local git_args=()
  local zmin_args=()
  for arg in "$@"; do
    case "$arg" in
      __OUT__) git_args+=("$git_out"); zmin_args+=("$zmin_out") ;;
      *) git_args+=("$arg"); zmin_args+=("$arg") ;;
    esac
  done

  set +e
  "$GIT_BIN" -C "$git_repo" diagnose "${git_args[@]}" >"$root/git.stdout" 2>"$root/git.stderr"
  git_exit=$?
  "$ZMIN_BIN" -C "$zmin_repo" diagnose "${zmin_args[@]}" >"$root/zmin.stdout" 2>"$root/zmin.stderr"
  zmin_exit=$?
  set -e

  local git_archive="$git_out/$expected_file"
  local zmin_archive="$zmin_out/$expected_file"
  test "$git_exit" = 0
  test "$zmin_exit" = 0
  test -f "$git_archive"
  test -f "$zmin_archive"
  list_zip "$git_archive" >"$root/git.zip"
  list_zip "$zmin_archive" >"$root/zmin.zip"
  normalize_text "$root/git.stdout" "$root" >"$root/git.stdout.norm"
  normalize_text "$root/zmin.stdout" "$root" >"$root/zmin.stdout.norm"
  normalize_text "$root/git.stderr" "$root" >"$root/git.stderr.norm"
  normalize_text "$root/zmin.stderr" "$root" >"$root/zmin.stderr.norm"
  normalize_text "$root/git.zip" "$root" >"$root/git.zip.norm"
  normalize_text "$root/zmin.zip" "$root" >"$root/zmin.zip.norm"

  cmp -s "$root/git.stdout.norm" "$root/zmin.stdout.norm"
  cmp -s "$root/git.stderr.norm" "$root/zmin.stderr.norm"
  cmp -s "$root/git.zip.norm" "$root/zmin.zip.norm"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

run_oracle diagnose_mode_all git-diagnostics-mode-all.zip --output-directory __OUT__ --suffix mode-all --mode all
run_oracle diagnose_output_directory_long git-diagnostics-out-long.zip --output-directory __OUT__ --suffix out-long
run_oracle diagnose_suffix_long git-diagnostics-suffix-long.zip --output-directory __OUT__ --suffix suffix-long
run_oracle diagnose_output_directory_short git-diagnostics-out-short.zip -o __OUT__ --suffix out-short
run_oracle diagnose_suffix_short git-diagnostics-suffix-short.zip -o __OUT__ -s suffix-short
