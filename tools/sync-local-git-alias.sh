#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

target_dir="$(
  awk -F '"' '/^[[:space:]]*target-dir[[:space:]]*=[[:space:]]*"/ { print $2; exit }' \
    "$repo_root/.cargo/config.toml"
)"
if [[ -z "${target_dir:-}" ]]; then
  echo "could not resolve cargo target-dir from $repo_root/.cargo/config.toml" >&2
  exit 1
fi

source_bin="${ZMIN_SOURCE_BIN:-$target_dir/release/zmin}"
dest_bin="${ZMIN_LOCAL_GIT_BIN:-$HOME/.local/bin/git.zmin-bin}"
stock_git="${ZMIN_STOCK_GIT:-${GIT_BIN:-/usr/bin/git}}"
verify_repo="${ZMIN_VERIFY_REPO:-$repo_root}"

if [[ ! -x "$source_bin" ]]; then
  echo "source binary is not executable: $source_bin" >&2
  echo "build it first with: cargo build --release -p zmin-cli --bin zmin" >&2
  exit 1
fi

mkdir -p "$(dirname "$dest_bin")"
tmp_bin="$(mktemp "$(dirname "$dest_bin")/.git.zmin-bin.tmp.XXXXXX")"
trap 'rm -f "$tmp_bin"' EXIT

cp "$source_bin" "$tmp_bin"
chmod +x "$tmp_bin"
mv -f "$tmp_bin" "$dest_bin"

version="$("$dest_bin" --version)"
if [[ "$version" != *zmin* ]]; then
  echo "installed binary does not report a zmin version: $dest_bin" >&2
  echo "$version" >&2
  exit 1
fi

src_hash="$(shasum -a 256 "$source_bin" | awk '{print $1}')"
dest_hash="$(shasum -a 256 "$dest_bin" | awk '{print $1}')"

if [[ "$src_hash" != "$dest_hash" ]]; then
  echo "installed binary hash does not match source binary" >&2
  echo "source: $source_bin $src_hash" >&2
  echo "dest:   $dest_bin $dest_hash" >&2
  exit 1
fi

if [[ ! -x "$stock_git" ]]; then
  echo "stock git is not executable: $stock_git" >&2
  exit 1
fi

if "$stock_git" --version | grep -qi 'zmin'; then
  echo "stock git resolved to a zmin binary: $stock_git" >&2
  exit 1
fi

if [[ -d "$verify_repo/.git" || -f "$verify_repo/.git" ]]; then
  verify_dir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-local-alias-verify.XXXXXX")"
  trap 'rm -f "$tmp_bin"; rm -rf "$verify_dir"' EXIT

  printf 'HEAD\nHEAD~1\n' >"$verify_dir/show.stdin"
  "$stock_git" -C "$verify_repo" \
    -c credential.helper= \
    -c core.quotepath=false \
    -c log.showSignature=false \
    show --name-status "--format=%H %P" --stdin \
    <"$verify_dir/show.stdin" >"$verify_dir/stock-show.out"
  "$dest_bin" -C "$verify_repo" \
    -c credential.helper= \
    -c core.quotepath=false \
    -c log.showSignature=false \
    show --name-status "--format=%H %P" --stdin \
    <"$verify_dir/show.stdin" >"$verify_dir/zmin-show.out"
  if ! cmp -s "$verify_dir/stock-show.out" "$verify_dir/zmin-show.out"; then
    echo "post-sync observed show --stdin output mismatch for $verify_repo" >&2
    diff -u "$verify_dir/stock-show.out" "$verify_dir/zmin-show.out" >&2 || true
    exit 1
  fi

  "$stock_git" -C "$verify_repo" \
    -c credential.helper= \
    -c core.quotepath=false \
    -c log.showSignature=false \
    status --porcelain -z --no-renames --untracked-files=all --ignored=matching -- \
    >"$verify_dir/stock-status.out"
  "$dest_bin" -C "$verify_repo" \
    -c credential.helper= \
    -c core.quotepath=false \
    -c log.showSignature=false \
    status --porcelain -z --no-renames --untracked-files=all --ignored=matching -- \
    >"$verify_dir/zmin-status.out"
  if ! cmp -s "$verify_dir/stock-status.out" "$verify_dir/zmin-status.out"; then
    echo "post-sync observed status output mismatch for $verify_repo" >&2
    exit 1
  fi
fi

printf 'synced %s -> %s\n' "$source_bin" "$dest_bin"
printf 'version: %s\n' "$version"
printf 'sha256: %s\n' "$dest_hash"
if [[ -d "$verify_repo/.git" || -f "$verify_repo/.git" ]]; then
  printf 'verified observed show/status on: %s\n' "$verify_repo"
fi
