#!/usr/bin/env bash
set -euo pipefail

repo="${ZMIN_GITHUB_REPO:-dschewchenko/zmin}"
readme="${ZMIN_RELEASE_README:-README.md}"
release_base="${ZMIN_RELEASE_BASE_URL:-https://github.com/$repo/releases/download}"
api_base="${ZMIN_GITHUB_API_URL:-https://api.github.com}"
metadata_json="$(mktemp)"
downloads_tsv="$(mktemp)"
api_error_file="$(mktemp)"

cleanup() {
  rm -f "$metadata_json" "$downloads_tsv" "$api_error_file"
}
trap cleanup EXIT

current_preview_tag() {
  awk '
    /^Current preview:[[:space:]]*/ {
      value = $0
      if (match(value, /`[^`]+`/)) {
        print substr(value, RSTART + 1, RLENGTH - 2)
        exit
      }
    }
  ' "$readme"
}

tag="${1:-$(current_preview_tag)}"
if [[ -z "$tag" ]]; then
  echo "release tag argument is required when $readme has no Current preview line" >&2
  exit 2
fi

expected_assets=(
  "zmin-x86_64-unknown-linux-gnu.tar.gz"
  "zmin-aarch64-unknown-linux-gnu.tar.gz"
  "zmin-x86_64-apple-darwin.tar.gz"
  "zmin-aarch64-apple-darwin.tar.gz"
  "zmin-x86_64-pc-windows-msvc.zip"
  "zmin-aarch64-pc-windows-msvc.zip"
  "SHA256SUMS"
)

api_headers=(
  -H "Accept: application/vnd.github+json"
  -H "User-Agent: zmin-release-check"
)

token="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
if [[ -n "$token" ]]; then
  api_headers+=(-H "Authorization: Bearer $token")
fi

downloads_available=0
if curl -fsSL "${api_headers[@]}" \
  "$api_base/repos/$repo/releases/tags/$tag" \
  -o "$metadata_json" 2>"$api_error_file"; then
  python3 - "$metadata_json" >"$downloads_tsv" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    release = json.load(handle)

for asset in release.get("assets", []):
    print(f"{asset.get('name', '')}\t{asset.get('download_count', '')}")
PY
  downloads_available=1
else
  echo "warning: GitHub API metadata unavailable; download counts are unknown" >&2
  if [[ -s "$api_error_file" ]]; then
    sed 's/^/warning: /' "$api_error_file" >&2
  fi
fi

download_count() {
  local asset="$1"
  if [[ "$downloads_available" -ne 1 ]]; then
    printf 'unknown'
    return
  fi
  awk -F '\t' -v asset="$asset" '$1 == asset { print $2; found = 1; exit } END { if (!found) print "unknown" }' "$downloads_tsv"
}

missing=0
printf 'Release assets for %s/%s\n' "$repo" "$tag"
printf '%-45s %s\n' "asset" "downloads"
for asset in "${expected_assets[@]}"; do
  if curl -fsIL "$release_base/$tag/$asset" >/dev/null; then
    printf '%-45s %s\n' "$asset" "$(download_count "$asset")"
  else
    printf '%-45s missing\n' "$asset"
    missing=1
  fi
done

if [[ "$missing" -ne 0 ]]; then
  exit 1
fi
