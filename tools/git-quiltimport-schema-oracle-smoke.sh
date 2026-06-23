#!/usr/bin/env bash
set -euo pipefail

ZMIN_BIN="${ZMIN_BIN:-target/release/zmin}"
GIT_BIN="${GIT_BIN:-/usr/bin/git}"
case "$ZMIN_BIN" in
  /*) ;;
  *) ZMIN_BIN="$PWD/$ZMIN_BIN" ;;
esac

tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-quiltimport-oracle.XXXXXX")"
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

make_seed_repo() {
  local repo="$1"
  mkdir "$repo"
  "$GIT_BIN" -C "$repo" init -q -b main
  "$GIT_BIN" -C "$repo" config user.name "Oracle"
  "$GIT_BIN" -C "$repo" config user.email "oracle@example.test"
  printf 'base\n' >"$repo/file.txt"
  "$GIT_BIN" -C "$repo" add -A
  GIT_AUTHOR_DATE="2030-01-01T00:00:00 +0000" \
    GIT_COMMITTER_DATE="2030-01-01T00:00:00 +0000" \
    "$GIT_BIN" -C "$repo" commit -qm "base"
  write_quilt_fixture "$repo"
}

write_quilt_fixture() {
  local repo="$1"
  mkdir -p "$repo/patches"
  printf 'change-one.patch\nadd-second.patch\n' >"$repo/patches/series"
  printf 'add-second.patch\nchange-one.patch\n' >"$repo/patches/reversed-series"
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

run_case() {
  local name="$1"
  shift
  local git_work="$tmpdir/${name}.git"
  local zmin_work="$tmpdir/${name}.zmin"
  local git_exit=0
  local zmin_exit=0

  cp -R "$base_seed" "$git_work"
  cp -R "$base_seed" "$zmin_work"

  set +e
  (
    cd "$git_work"
    GIT_AUTHOR_DATE="2030-01-02T00:00:00 +0000" \
      GIT_COMMITTER_DATE="2030-01-02T00:00:00 +0000" \
      "$GIT_BIN" quiltimport "$@"
  ) >"$tmpdir/${name}.git.out" 2>"$tmpdir/${name}.git.err"
  git_exit=$?
  (
    cd "$zmin_work"
    GIT_AUTHOR_DATE="2030-01-02T00:00:00 +0000" \
      GIT_COMMITTER_DATE="2030-01-02T00:00:00 +0000" \
      "$ZMIN_BIN" quiltimport "$@"
  ) >"$tmpdir/${name}.zmin.out" 2>"$tmpdir/${name}.zmin.err"
  zmin_exit=$?
  set -e

  if [ "$git_exit" != "$zmin_exit" ]; then
    echo "$name exit differs: stock=$git_exit zmin=$zmin_exit" >&2
    echo "stock stderr:" >&2
    sed 's/^/  /' "$tmpdir/${name}.git.err" >&2 || true
    echo "zmin stderr:" >&2
    sed 's/^/  /' "$tmpdir/${name}.zmin.err" >&2 || true
    return 1
  fi
  compare_files stdout "$tmpdir/${name}.git.out" "$tmpdir/${name}.zmin.out"
  compare_files stderr "$tmpdir/${name}.git.err" "$tmpdir/${name}.zmin.err"
  "$GIT_BIN" -C "$git_work" log --format='%an <%ae>|%s|%b' --reverse >"$tmpdir/${name}.git.log"
  "$GIT_BIN" -C "$zmin_work" log --format='%an <%ae>|%s|%b' --reverse >"$tmpdir/${name}.zmin.log"
  compare_files log "$tmpdir/${name}.git.log" "$tmpdir/${name}.zmin.log"
  "$GIT_BIN" -C "$git_work" rev-parse 'HEAD^{tree}' >"$tmpdir/${name}.git.tree"
  "$GIT_BIN" -C "$zmin_work" rev-parse 'HEAD^{tree}' >"$tmpdir/${name}.zmin.tree"
  compare_files tree "$tmpdir/${name}.git.tree" "$tmpdir/${name}.zmin.tree"
  "$GIT_BIN" -C "$git_work" status --porcelain=v1 -uno >"$tmpdir/${name}.git.status"
  "$GIT_BIN" -C "$zmin_work" status --porcelain=v1 -uno >"$tmpdir/${name}.zmin.status"
  compare_files status "$tmpdir/${name}.git.status" "$tmpdir/${name}.zmin.status"
  printf '%s\tok\texit=%s\n' "$name" "$git_exit"
}

base_seed="$tmpdir/base"
make_seed_repo "$base_seed"

run_case quiltimport_dry_run_long --dry-run --author "Patch Author <patch@example.test>" --patches patches
