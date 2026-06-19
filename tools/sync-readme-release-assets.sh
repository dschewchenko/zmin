#!/usr/bin/env bash
set -euo pipefail

mode="${1:---write}"
workflow="${ZMIN_RELEASE_WORKFLOW:-.github/workflows/release-artifacts.yml}"
readme="${ZMIN_RELEASE_README:-README.md}"
release_base="${ZMIN_RELEASE_BASE_URL:-https://github.com/dschewchenko/zmin/releases/download}"
start_marker="<!-- zmin-release-assets:start -->"
end_marker="<!-- zmin-release-assets:end -->"

if [[ "$mode" != "--write" && "$mode" != "--check" ]]; then
  echo "usage: tools/sync-readme-release-assets.sh [--write|--check]" >&2
  exit 2
fi

assets="$(
  awk '
    /^[[:space:]]*artifact:[[:space:]]*/ {
      value = $0
      sub(/^[[:space:]]*artifact:[[:space:]]*/, "", value)
      sub(/[[:space:]]*#.*/, "", value)
      gsub(/^["'\'']|["'\'']$/, "", value)
      if (value != "") {
        print value
      }
    }
  ' "$workflow"
)"

if [[ -z "$assets" ]]; then
  echo "no release artifacts found in $workflow" >&2
  exit 1
fi

release_tag="${ZMIN_RELEASE_TAG:-}"
if [[ -z "$release_tag" ]]; then
  release_tag="$(
    awk '
      /^Current preview:[[:space:]]*/ {
        value = $0
        if (match(value, /`[^`]+`/)) {
          print substr(value, RSTART + 1, RLENGTH - 2)
          exit
        }
      }
    ' "$readme"
  )"
fi

if [[ -z "$release_tag" ]]; then
  echo "no release tag found in $readme" >&2
  exit 1
fi

replacement="$start_marker"$'\n'
while IFS= read -r asset; do
  replacement+="- [\`$asset\`]($release_base/$release_tag/$asset)"$'\n'
done <<<"$assets"
replacement+="- [\`SHA256SUMS\`]($release_base/$release_tag/SHA256SUMS)"$'\n'
replacement+="$end_marker"

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

START_MARKER="$start_marker" \
END_MARKER="$end_marker" \
REPLACEMENT="$replacement" \
  perl -0pe '
    BEGIN {
      our $count = 0;
      our $start = quotemeta $ENV{"START_MARKER"};
      our $end = quotemeta $ENV{"END_MARKER"};
    }

    our $count;
    our $start;
    our $end;
    $count += s/$start.*?$end/$ENV{"REPLACEMENT"}/s;

    END {
      exit($count ? 0 : 3);
    }
  ' "$readme" >"$tmp" || {
  code=$?
  if [[ "$code" -eq 3 ]]; then
    echo "missing release asset markers in $readme" >&2
  fi
  exit "$code"
}

if cmp -s "$tmp" "$readme"; then
  echo "README release asset list is in sync"
  exit 0
fi

if [[ "$mode" == "--check" ]]; then
  echo "README release asset list is out of sync; run tools/sync-readme-release-assets.sh" >&2
  exit 1
fi

mv "$tmp" "$readme"
trap - EXIT
echo "updated README release asset list"
