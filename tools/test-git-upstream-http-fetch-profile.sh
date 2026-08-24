#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
validator="$repo_root/tools/git-upstream-http-provenance.sh"
cache_root="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
cache_root="$(cd "$cache_root" && pwd -P)"
tag=v2.55.0
commit=e9019fcafe0040228b8631c30f97ae1adb61bcdc
platform="$(uname -s)"
arch="$(uname -m)"
base_bundle="$cache_root/http-bundle-$tag-$commit-$platform-$arch"
pinned_bundle="$cache_root/http-bundle-$tag-$commit-$platform-$arch-with-http-fetch-pinned"
make_path="${ZMIN_HTTP_MAKE:-/usr/bin/make}"
cc_path="${ZMIN_HTTP_CC:-/usr/bin/cc}"
ar_path="${ZMIN_HTTP_AR:-/usr/bin/ar}"
ranlib_path="${ZMIN_HTTP_RANLIB:-/usr/bin/ranlib}"

[[ "$platform" == Darwin && "$arch" == arm64 ]] || {
  echo 'with-http-fetch-pinned profile test requires Darwin arm64' >&2
  exit 2
}
for required in "$make_path" "$cc_path" "$ar_path" "$ranlib_path"; do
  [[ "$required" == /* && -f "$required" && ! -L "$required" && -x "$required" ]] || {
    echo "profile test requires an absolute regular executable: $required" >&2
    exit 2
  }
done
[[ -d "$base_bundle" && -d "$pinned_bundle" ]] || {
  echo 'profile test requires both canonical and pinned cache bundles' >&2
  exit 2
}

run_validate() {
  local bundle="$1" case_cache="${2:-$cache_root}"
  ZMIN_UPSTREAM_GIT_CACHE="$case_cache" \
    ZMIN_HTTP_BUNDLE_PROFILE=with-http-fetch-pinned \
    ZMIN_GIT_HTTP_BUNDLE="$bundle" \
    ZMIN_STOCK_GIT="$bundle/git" \
    "$validator" validate with-http-fetch-pinned
}

ZMIN_UPSTREAM_GIT_CACHE="$cache_root" \
  ZMIN_GIT_HTTP_BUNDLE="$base_bundle" ZMIN_STOCK_GIT="$base_bundle/git" \
  "$validator" validate canonical >/dev/null
validated_pinned="$(run_validate "$pinned_bundle" 2>/dev/null)"
[[ "$validated_pinned" == "$pinned_bundle" ]] || {
  echo 'pinned profile validation resolved an unexpected bundle path' >&2
  exit 1
}
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/zmin-http-fetch-profile.XXXXXX")"
tmp_root="$(cd "$tmp_root" && pwd -P)"
cleanup() {
  chmod -R u+w "$tmp_root" 2>/dev/null || :
  rm -rf "$tmp_root"
}
trap cleanup EXIT
expected_fetch_sha=fc2e8b9e47cafb39140ca90f56fbc6cb09912c6d715feb020157733868f08b0e
for build_number in one two; do
  clean_cache="$tmp_root/cache-$build_number"
  mkdir -p "$clean_cache"
  cp -R "$cache_root/git-v2.55.0" "$clean_cache/git-v2.55.0"
  cp "$cache_root/v2.55.0.tar.gz" "$clean_cache/v2.55.0.tar.gz"
  cp -R "$base_bundle" "$clean_cache/$(basename "$base_bundle")"
  chmod -R a-w "$clean_cache/git-v2.55.0" "$clean_cache/$(basename "$base_bundle")"
  clean_bundle="$clean_cache/$(basename "$pinned_bundle")"
  build_output="$(ZMIN_UPSTREAM_GIT_CACHE="$clean_cache" \
    ZMIN_HTTP_MAKE="$make_path" ZMIN_HTTP_CC="$cc_path" ZMIN_HTTP_AR="$ar_path" \
    ZMIN_HTTP_RANLIB="$ranlib_path" ZMIN_HTTP_BUNDLE_PROFILE=with-http-fetch-pinned \
    "$validator" build with-http-fetch-pinned 2>"$tmp_root/build-$build_number.log")"
  [[ "$build_output" == "$clean_bundle" ]] || {
    echo "clean pinned build $build_number returned unexpected output: $build_output" >&2
    exit 1
  }
  [[ "$(/usr/bin/shasum -a 256 "$clean_bundle/git-http-fetch" | awk '{ print $1 }')" == "$expected_fetch_sha" ]] || {
    echo "clean pinned build $build_number produced an unexpected helper" >&2
    exit 1
  }
  [[ "$(stat -f '%04Lp' "$clean_bundle/git-http-fetch")" == 0555 ]] || {
    echo "clean pinned build $build_number did not publish helper mode 0555" >&2
    exit 1
  }
  run_validate "$clean_bundle" "$clean_cache" >/dev/null 2>"$tmp_root/validate-$build_number.log"
done
cmp "$tmp_root/cache-one/$(basename "$pinned_bundle")/git-http-fetch" \
  "$tmp_root/cache-two/$(basename "$pinned_bundle")/git-http-fetch"

manifest_fixture_cache="$tmp_root/cache-manifest"
mkdir -p "$manifest_fixture_cache"
cp -R "$cache_root/git-v2.55.0" "$manifest_fixture_cache/git-v2.55.0"
cp "$cache_root/v2.55.0.tar.gz" "$manifest_fixture_cache/v2.55.0.tar.gz"
manifest_fixture="$manifest_fixture_cache/$(basename "$pinned_bundle")"
cp -R "$pinned_bundle" "$manifest_fixture"
chmod -R u+w "$manifest_fixture"
awk -F '\t' -v source_root="$manifest_fixture_cache/git-v2.55.0" 'BEGIN { OFS = "\t" } $1 == "source_root" { $2 = source_root } { print }' \
  "$manifest_fixture/manifest.tsv" >"$manifest_fixture/manifest.tsv.new"
mv "$manifest_fixture/manifest.tsv.new" "$manifest_fixture/manifest.tsv"
awk -F '\t' -v sha=0000000000000000000000000000000000000000000000000000000000000000 \
  'BEGIN { OFS = "\t" } $1 == "member" && $2 == "git-http-fetch" { $5 = sha } { print }' \
  "$manifest_fixture/manifest.tsv" >"$manifest_fixture/manifest.tsv.new"
mv "$manifest_fixture/manifest.tsv.new" "$manifest_fixture/manifest.tsv"
/usr/bin/shasum -a 256 "$manifest_fixture/manifest.tsv" | awk '{ print $1 }' >"$manifest_fixture/manifest.tsv.sha256"
chmod -R a-w "$manifest_fixture"
set +e
manifest_output="$(run_validate "$manifest_fixture" "$manifest_fixture_cache" 2>&1)"
manifest_status=$?
set -e
[[ "$manifest_status" -ne 0 && "$manifest_output" == *'manifest digest is not the trusted source-built helper'* ]] || {
  printf '%s\n' "$manifest_output" >&2
  echo 'rewritten helper manifest digest was not rejected by the pinned profile' >&2
  exit 1
}

fixture_cache="$tmp_root/cache"
mkdir -p "$fixture_cache"
cp -R "$cache_root/git-v2.55.0" "$fixture_cache/git-v2.55.0"
cp "$cache_root/v2.55.0.tar.gz" "$fixture_cache/v2.55.0.tar.gz"
fixture="$fixture_cache/$(basename "$pinned_bundle")"
cp -R "$pinned_bundle" "$fixture"
chmod -R u+w "$fixture"
awk -F '\t' -v source_root="$fixture_cache/git-v2.55.0" 'BEGIN { OFS = "\t" } $1 == "source_root" { $2 = source_root } { print }' \
  "$fixture/manifest.tsv" >"$fixture/manifest.tsv.new"
mv "$fixture/manifest.tsv.new" "$fixture/manifest.tsv"
/usr/bin/shasum -a 256 "$fixture/manifest.tsv" | awk '{ print $1 }' >"$fixture/manifest.tsv.sha256"
cp "$tmp_root/cache-one/$(basename "$pinned_bundle")/git-http-fetch" "$fixture/git-http-fetch"
printf '\377' | dd of="$fixture/git-http-fetch" bs=1 seek=1000 count=1 conv=notrunc 2>/dev/null
awk -F '\t' -v sha="$expected_fetch_sha" 'BEGIN { OFS = "\t" } $1 == "member" && $2 == "git-http-fetch" { $5 = sha } { print }' \
  "$fixture/manifest.tsv" >"$fixture/manifest.tsv.new"
mv "$fixture/manifest.tsv.new" "$fixture/manifest.tsv"
chmod 555 "$fixture/git-http-fetch"
/usr/bin/shasum -a 256 "$fixture/manifest.tsv" | awk '{ print $1 }' >"$fixture/manifest.tsv.sha256"
chmod -R a-w "$fixture"
fake_perl="$tmp_root/fake-perl"
{
  printf '#!/bin/sh\n'
  printf 'last=""\n'
  printf 'for arg do last="$arg"; done\n'
  printf 'case "$*" in *-MFile::Find*) exec /usr/bin/perl "$@";; esac\n'
  printf 'case "$last" in *git-http-fetch) printf '\''%%s\\n'\'' '\''%s'\''; exit 0;; esac\n' "$expected_fetch_sha"
  printf 'exec /usr/bin/perl "$@"\n'
} >"$fake_perl"
chmod 755 "$fake_perl"
set +e
fake_perl_output="$(ZMIN_HTTP_PERL="$fake_perl" ZMIN_UPSTREAM_GIT_CACHE="$fixture_cache" \
  ZMIN_HTTP_BUNDLE_PROFILE=with-http-fetch-pinned ZMIN_GIT_HTTP_BUNDLE="$fixture" \
  ZMIN_STOCK_GIT="$fixture/git" "$validator" validate with-http-fetch-pinned 2>&1)"
fake_perl_status=$?
set -e
[[ "$fake_perl_status" -ne 0 && "$fake_perl_output" == *'rejects the unbound Perl environment variable: ZMIN_HTTP_PERL'* ]] || {
  printf '%s\n' "$fake_perl_output" >&2
  echo 'fake Perl hash override was not rejected by the pinned profile' >&2
  exit 1
}
set +e
tampered_output="$(run_validate "$fixture" "$fixture_cache" 2>&1)"
tampered_status=$?
set -e
[[ "$tampered_status" -ne 0 && "$tampered_output" == *'pinned helper does not match a fresh deterministic source/toolchain rebuild'* ]] || {
  printf '%s\n' "$tampered_output" >&2
  echo 'tampered source-built helper was not rejected by the pinned profile' >&2
  exit 1
}

printf 'http-fetch-profile-tests=valid+two-clean-builds-identical+tampered-helper-rejected\n'
