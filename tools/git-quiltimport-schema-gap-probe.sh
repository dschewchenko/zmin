#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-quiltimport-gap.XXXXXX")"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.test"
  printf 'base\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add -A
  GIT_AUTHOR_DATE="1893456000 +0000" \
    GIT_COMMITTER_DATE="1893456000 +0000" \
    "$GIT_BIN" -C "$repo" commit -qm "base"
  mkdir -p "$repo/patches"
  printf 'change-one.patch\nadd-second.patch\n' >"$repo/explicit-series"
  cat >"$repo/patches/change-one.patch" <<'PATCH'
Change first file

More body.
---
diff --git a/file.txt b/file.txt
index df967b9..ce01362 100644
--- a/file.txt
+++ b/file.txt
@@ -1 +1 @@
-base
+changed
PATCH
  cat >"$repo/patches/add-second.patch" <<'PATCH'
Add second file
---
diff --git a/second.txt b/second.txt
new file mode 100644
index 0000000..e019be0
--- /dev/null
+++ b/second.txt
@@ -0,0 +1 @@
+second
PATCH
}

make_keep_non_patch_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.test"
  printf 'base\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add -A
  GIT_AUTHOR_DATE="1893456000 +0000" \
    GIT_COMMITTER_DATE="1893456000 +0000" \
    "$GIT_BIN" -C "$repo" commit -qm "base"
  mkdir -p "$repo/patches"
  printf 'note.txt\nchange-one.patch\n' >"$repo/patches/series"
  printf 'not a patch\n' >"$repo/patches/note.txt"
  cat >"$repo/patches/change-one.patch" <<'PATCH'
Change first file
---
diff --git a/file.txt b/file.txt
index df967b9..ce01362 100644
--- a/file.txt
+++ b/file.txt
@@ -1 +1 @@
-base
+changed
PATCH
}

run_probe() {
  local name="$1"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  cp -R "$base_seed" "$git_work"
  cp -R "$base_seed" "$zmin_work"

  set +e
  (
    cd "$git_work"
    GIT_AUTHOR_DATE="1893542400 +0000" \
      GIT_COMMITTER_DATE="1893542400 +0000" \
      "$GIT_BIN" quiltimport --author "Patch Author <patch@example.test>" --series explicit-series
  ) >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (
    cd "$zmin_work"
    GIT_AUTHOR_DATE="1893542400 +0000" \
      GIT_COMMITTER_DATE="1893542400 +0000" \
      "$ZMIN_BIN" quiltimport --author "Patch Author <patch@example.test>" --series explicit-series
  ) >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 4
  test "$zmin_exit" = 0
  grep -F "error: file.txt: does not match index" "$tmpdir/${name}.git.err" >/dev/null
  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

run_keep_non_patch_probe() {
  local name="$1"
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  cp -R "$keep_non_patch_seed" "$git_work"
  cp -R "$keep_non_patch_seed" "$zmin_work"

  set +e
  (
    cd "$git_work"
    GIT_AUTHOR_DATE="1893542400 +0000" \
      GIT_COMMITTER_DATE="1893542400 +0000" \
      "$GIT_BIN" quiltimport --keep-non-patch --author "Patch Author <patch@example.test>" --patches patches
  ) >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (
    cd "$zmin_work"
    GIT_AUTHOR_DATE="1893542400 +0000" \
      GIT_COMMITTER_DATE="1893542400 +0000" \
      "$ZMIN_BIN" quiltimport --keep-non-patch --author "Patch Author <patch@example.test>" --patches patches
  ) >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  test "$git_exit" = 1
  test "$zmin_exit" = 128
  grep -F "Patch is empty.  Was it split wrong?" "$tmpdir/${name}.git.out" >/dev/null
  grep -F "fatal: No valid patches in input" "$tmpdir/${name}.zmin.err" >/dev/null
  printf '%s\tgap\tstock_exit=%s\tzmin_exit=%s\n' "$name" "$git_exit" "$zmin_exit"
}

base_seed="$tmpdir/base"
make_seed_repo "$base_seed"
keep_non_patch_seed="$tmpdir/keep-non-patch-base"
make_keep_non_patch_seed_repo "$keep_non_patch_seed"
run_probe quiltimport_series_explicit_gap
run_keep_non_patch_probe quiltimport_keep_non_patch_gap
