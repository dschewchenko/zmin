#!/usr/bin/env bash
set -euo pipefail

repo="${ZMIN_GITHUB_REPO:-dschewchenko/zmin}"
tag="${1:-v0.0.1-preview.20260619}"
assets_file="$(mktemp)"
api_error_file="$(mktemp)"

cleanup() {
  rm -f "$assets_file" "$api_error_file"
}
trap cleanup EXIT

expected_assets=(
  "zmin-x86_64-unknown-linux-gnu.tar.gz"
  "zmin-aarch64-unknown-linux-gnu.tar.gz"
  "zmin-x86_64-apple-darwin.tar.gz"
  "zmin-aarch64-apple-darwin.tar.gz"
  "zmin-x86_64-pc-windows-msvc.zip"
  "zmin-aarch64-pc-windows-msvc.zip"
  "SHA256SUMS"
)

if ! gh api "repos/$repo/releases/tags/$tag" \
  --jq '.assets[] | [.name, (.download_count | tostring)] | @tsv' \
  >"$assets_file" 2>"$api_error_file"; then
  echo "release not found or inaccessible: $repo/$tag" >&2
  exit 1
fi

missing=0
printf 'Release assets for %s/%s\n' "$repo" "$tag"
printf '%-45s %s\n' "asset" "downloads"
for asset in "${expected_assets[@]}"; do
  if awk -F '\t' -v asset="$asset" '$1 == asset { found = 1 } END { exit found ? 0 : 1 }' "$assets_file"; then
    downloads="$(awk -F '\t' -v asset="$asset" '$1 == asset { print $2; exit }' "$assets_file")"
    printf '%-45s %s\n' "$asset" "$downloads"
  else
    printf '%-45s missing\n' "$asset"
    missing=1
  fi
done

if [[ "$missing" -ne 0 ]]; then
  exit 1
fi
