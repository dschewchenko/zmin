#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
validator="$repo_root/tools/git-upstream-http-provenance.sh"
cache_root="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
bundle="${ZMIN_GIT_HTTP_BUNDLE:-}"
make_path="${ZMIN_HTTP_MAKE:-}"
perl_path="${ZMIN_HTTP_PERL:-$(command -v perl 2>/dev/null || true)}"
[[ "$make_path" == /* && -f "$make_path" && ! -L "$make_path" && -x "$make_path" ]] || {
  echo "ZMIN_HTTP_MAKE must select an explicit absolute regular executable" >&2
  exit 2
}
[[ "$perl_path" == /* && -f "$perl_path" && ! -L "$perl_path" && -x "$perl_path" ]] || {
  echo "ZMIN_HTTP_PERL must select an explicit absolute regular executable" >&2
  exit 2
}
"$perl_path" -MDigest::SHA -e 'Digest::SHA::sha256_hex("")' >/dev/null 2>&1 || {
  echo "ZMIN_HTTP_PERL must provide Digest::SHA" >&2
  exit 2
}
[[ "$bundle" == /* && -d "$bundle" && ! -L "$bundle" ]] || {
  echo "set ZMIN_GIT_HTTP_BUNDLE to the validated bundle for this test" >&2
  exit 2
}
cache_root="$(cd "$cache_root" && pwd -P)"
bundle="$(cd "$bundle" && pwd -P)"
git_member="$(awk -F '\t' '$1 == "member" && $2 == "git" { print $3; count += 1 } END { if (count != 1) exit 1 }' "$bundle/bundle.tsv")" || {
  echo 'validated bundle has no unique Git member' >&2
  exit 2
}
backend_member="$(awk -F '\t' '$1 == "member" && $2 == "git-http-backend" { print $3; count += 1 } END { if (count != 1) exit 1 }' "$bundle/bundle.tsv")" || {
  echo 'validated bundle has no unique HTTP backend member' >&2
  exit 2
}
bundle_name="$(basename "$bundle")"
source_policy="$(awk -F '\t' '$1 == "source_identity_policy" { print $2; count += 1 } END { if (count != 1) exit 1 }' "$repo_root/tools/git-upstream-compat-contract.tsv")"
[[ "$source_policy" == 'archive_sha256_exact; tag_commit_declared; source_manifest_exact; no_checkout_fallback' ]] || {
  echo "source identity policy is not archive-only and manifest-bound" >&2
  exit 1
}
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/zmin-http-provenance-test.XXXXXX")"
tmp_root="$(cd "$tmp_root" && pwd -P)"
cleanup() {
  chmod -R u+w "$tmp_root" 2>/dev/null || :
  find "$tmp_root" -type d -exec chmod u+rwx {} + 2>/dev/null || :
  find "$tmp_root" -type f -exec chmod u+rw {} + 2>/dev/null || :
  rm -rf "$tmp_root"
}
trap cleanup EXIT

run_validate() {
  local case_cache="$1" case_bundle="$2" stock_path="$3"
  ZMIN_UPSTREAM_GIT_CACHE="$case_cache" \
    ZMIN_HTTP_PERL="$perl_path" ZMIN_HTTP_MAKE="$make_path" \
    ZMIN_GIT_HTTP_BUNDLE="$case_bundle" ZMIN_STOCK_GIT="$stock_path" \
    "$validator" validate
}

run_source_validate() {
  local case_cache="$1"
  ZMIN_UPSTREAM_GIT_CACHE="$case_cache" ZMIN_HTTP_PERL="$perl_path" \
    "$validator" validate-source
}

expect_failure() {
  local label="$1" expected="$2" case_cache="$3" case_bundle="$4" stock_path="$5"
  local output status
  set +e
  output="$(run_validate "$case_cache" "$case_bundle" "$stock_path" 2>&1)"
  status=$?
  set -e
  [[ "$status" -ne 0 ]] || { echo "$label unexpectedly passed" >&2; exit 1; }
  [[ "$output" == *"$expected"* ]] || {
    printf '%s\n' "$output" >&2
    echo "$label did not report $expected" >&2
    exit 1
  }
}

copy_fixture_cache() {
  local destination="$1"
  mkdir -p "$destination"
  cp -R "$cache_root/git-v2.55.0" "$destination/git-v2.55.0"
  cp "$cache_root/v2.55.0.tar.gz" "$destination/v2.55.0.tar.gz"
  cp -R "$bundle" "$destination/$bundle_name"
}

fixture_cache="$tmp_root/fixture-cache"
copy_fixture_cache "$fixture_cache"
fixture_bundle="$fixture_cache/$bundle_name"
fixture_git="$fixture_bundle/$git_member"
run_validate "$fixture_cache" "$fixture_bundle" "$fixture_git" >/dev/null
run_source_validate "$fixture_cache" >/dev/null

chmod -R u+w "$fixture_cache/git-v2.55.0"
printf 'tampered\n' >>"$fixture_cache/git-v2.55.0/command-list.txt"
chmod -R a-w "$fixture_cache/git-v2.55.0"
set +e
source_tamper_output="$(run_source_validate "$fixture_cache" 2>&1)"
source_tamper_status=$?
set -e
[[ "$source_tamper_status" -ne 0 && "$source_tamper_output" == *'validated pristine source manifest mismatch'* ]] || {
  printf '%s\n' "$source_tamper_output" >&2
  echo 'source-only tamper was not rejected by the canonical manifest verifier' >&2
  exit 1
}
chmod -R u+w "$fixture_cache/git-v2.55.0"
cp "$cache_root/git-v2.55.0/command-list.txt" "$fixture_cache/git-v2.55.0/command-list.txt"
chmod -R a-w "$fixture_cache/git-v2.55.0"

chmod -R u+w "$fixture_cache/git-v2.55.0"
printf 'extra\n' >"$fixture_cache/git-v2.55.0/source-extra"
chmod -R a-w "$fixture_cache/git-v2.55.0"
set +e
source_extra_output="$(run_source_validate "$fixture_cache" 2>&1)"
source_extra_status=$?
set -e
[[ "$source_extra_status" -ne 0 && "$source_extra_output" == *'validated pristine source manifest mismatch'* ]] || {
  printf '%s\n' "$source_extra_output" >&2
  echo 'source-only extra entry was not rejected by the canonical manifest verifier' >&2
  exit 1
}
chmod -R u+w "$fixture_cache/git-v2.55.0"
rm -f "$fixture_cache/git-v2.55.0/source-extra"
chmod -R a-w "$fixture_cache/git-v2.55.0"

mv "$fixture_cache/git-v2.55.0" "$fixture_cache/git-v2.55.0-real"
ln -s git-v2.55.0-real "$fixture_cache/git-v2.55.0"
set +e
source_symlink_output="$(run_source_validate "$fixture_cache" 2>&1)"
source_symlink_status=$?
set -e
[[ "$source_symlink_status" -ne 0 && "$source_symlink_output" == *'validated pristine Git v2.55.0 source root is incomplete or mutable'* ]] || {
  printf '%s\n' "$source_symlink_output" >&2
  echo 'source-only symlink root was not rejected' >&2
  exit 1
}
rm "$fixture_cache/git-v2.55.0"
mv "$fixture_cache/git-v2.55.0-real" "$fixture_cache/git-v2.55.0"

bad_perl="$tmp_root/no-digest-perl"
printf '#!/bin/sh\nexit 1\n' >"$bad_perl"
chmod +x "$bad_perl"
set +e
digest_output="$(ZMIN_UPSTREAM_GIT_CACHE="$fixture_cache" ZMIN_HTTP_PERL="$bad_perl" \
  ZMIN_GIT_HTTP_BUNDLE="$fixture_bundle" ZMIN_STOCK_GIT="$fixture_git" \
  "$validator" validate 2>&1)"
digest_status=$?
set -e
[[ "$digest_status" -ne 0 && "$digest_output" == *'ZMIN_HTTP_PERL must provide Digest::SHA'* ]] || {
  printf '%s\n' "$digest_output" >&2
  echo 'missing Digest::SHA was not rejected with its intended diagnostic' >&2
  exit 1
}

set +e
ambient_make_output="$(env -u ZMIN_HTTP_MAKE ZMIN_UPSTREAM_GIT_CACHE="$cache_root" \
  ZMIN_HTTP_PERL="$perl_path" "$validator" build 2>&1)"
ambient_make_rc=$?
set -e
[[ "$ambient_make_rc" -ne 0 && "$ambient_make_output" == *'ZMIN_HTTP_MAKE must select an absolute regular executable'* ]] || {
  printf '%s\n' "$ambient_make_output" >&2
  echo 'ambient Make fallback was not rejected' >&2
  exit 1
}

marker_source="$tmp_root/marker-cache/git-v2.55.0"
mkdir -p "$marker_source/Documentation"
cp "$cache_root/v2.55.0.tar.gz" "$tmp_root/marker-cache/v2.55.0.tar.gz"
cp -R "$bundle" "$tmp_root/marker-cache/$bundle_name"
cp "$cache_root/git-v2.55.0/GIT-VERSION-GEN" "$marker_source/GIT-VERSION-GEN"
cp "$cache_root/git-v2.55.0/command-list.txt" "$marker_source/command-list.txt"
printf '%064d\n' 0 >"$marker_source/.zmin-pristine-source.sha256"
chmod -R a-w "$tmp_root/marker-cache/git-v2.55.0"
expect_failure wrong-source-marker 'validated Git source marker does not match the frozen archive' \
  "$tmp_root/marker-cache" "$tmp_root/marker-cache/$bundle_name" "$tmp_root/marker-cache/$bundle_name/$git_member"

chmod -R u+w "$fixture_cache/git-v2.55.0"
expect_failure source-writable 'validated pristine Git source root is writable' \
  "$fixture_cache" "$fixture_bundle" "$fixture_git"
printf 'tampered\n' >>"$fixture_cache/git-v2.55.0/Documentation/.editorconfig"
chmod -R a-w "$fixture_cache/git-v2.55.0"
expect_failure source-tamper 'validated pristine source manifest mismatch' \
  "$fixture_cache" "$fixture_bundle" "$fixture_git"
chmod -R u+w "$fixture_cache/git-v2.55.0"
rm -rf "$fixture_cache/git-v2.55.0"
cp -R "$cache_root/git-v2.55.0" "$fixture_cache/git-v2.55.0"

mv "$fixture_cache/git-v2.55.0" "$fixture_cache/git-v2.55.0-real"
ln -s git-v2.55.0-real "$fixture_cache/git-v2.55.0"
expect_failure source-symlink 'validated pristine Git v2.55.0 source root is incomplete or mutable' \
  "$fixture_cache" "$fixture_bundle" "$fixture_git"
rm "$fixture_cache/git-v2.55.0"
mv "$fixture_cache/git-v2.55.0-real" "$fixture_cache/git-v2.55.0"

ln -s "$fixture_bundle" "$tmp_root/bundle-link"
expect_failure bundle-symlink 'HTTP comparator paths are not the canonical derived bundle paths' \
  "$fixture_cache" "$tmp_root/bundle-link" "$tmp_root/bundle-link/$git_member"
rm "$tmp_root/bundle-link"

chmod -R u+w "$fixture_bundle"
printf 'rogue\n' >"$fixture_bundle/rogue"
chmod -R a-w "$fixture_bundle"
expect_failure bundle-extra 'HTTP comparator bundle contains unexpected entry: rogue' \
  "$fixture_cache" "$fixture_bundle" "$fixture_git"
chmod -R u+w "$fixture_bundle"
rm -f "$fixture_bundle/rogue"

chmod -R u+w "$fixture_bundle"
printf 'x' | dd of="$fixture_bundle/$git_member" bs=1 seek=0 conv=notrunc >/dev/null 2>&1
chmod -R a-w "$fixture_bundle"
expect_failure member-checksum "HTTP comparator bundle member checksum mismatch: $git_member" \
  "$fixture_cache" "$fixture_bundle" "$fixture_git"
chmod -R u+w "$fixture_bundle"
rm -rf "$fixture_bundle"
cp -R "$bundle" "$fixture_bundle"

chmod -R u+w "$fixture_bundle"
awk -F '\t' 'BEGIN { OFS="\t" } $1 == "platform" { $2 = "forged" } { print }' \
  "$fixture_bundle/bundle.tsv" >"$fixture_bundle/bundle.tsv.new"
mv "$fixture_bundle/bundle.tsv.new" "$fixture_bundle/bundle.tsv"
chmod -R a-w "$fixture_bundle"
expect_failure manifest-checksum 'HTTP comparator bundle manifest checksum mismatch' \
  "$fixture_cache" "$fixture_bundle" "$fixture_git"
chmod -R u+w "$fixture_bundle"
rm -rf "$fixture_bundle"
cp -R "$bundle" "$fixture_bundle"

chmod -R u+w "$fixture_bundle"
rm "$fixture_bundle/$backend_member"
chmod -R a-w "$fixture_bundle"
expect_failure missing-member "HTTP comparator bundle member is missing or mutable: $backend_member" \
  "$fixture_cache" "$fixture_bundle" "$fixture_git"
chmod -R u+w "$fixture_bundle"
rm -rf "$fixture_bundle"
cp -R "$bundle" "$fixture_bundle"

expect_failure wrong-git-member 'HTTP comparator paths are not the canonical derived bundle paths' \
  "$fixture_cache" "$fixture_bundle" "$tmp_root/not-the-bundle-git"

lock_cache="$tmp_root/lock-cache"
mkdir -p "$lock_cache"
cp -R "$cache_root/git-v2.55.0" "$lock_cache/git-v2.55.0"
cp "$cache_root/v2.55.0.tar.gz" "$lock_cache/v2.55.0.tar.gz"
lock_dir="$lock_cache/.zmin-http-bundle.lock"
mkdir "$lock_dir"
printf 'foreign-builder\n' >"$lock_dir/owner"
lock_output=""
set +e
lock_output="$(ZMIN_UPSTREAM_GIT_CACHE="$lock_cache" ZMIN_HTTP_PERL="$perl_path" \
  ZMIN_HTTP_MAKE="$make_path" ZMIN_HTTP_LOCK_TIMEOUT_SECONDS=1 \
  "$validator" build 2>&1)"
lock_status=$?
set -e
[[ "$lock_status" -ne 0 && "$lock_output" == *'timed out waiting for HTTP comparator bundle lock'* ]] || {
  printf '%s\n' "$lock_output" >&2
  echo 'lock contention did not fail with its intended diagnostic' >&2
  exit 1
}
[[ "$(cat "$lock_dir/owner")" == foreign-builder ]] || {
  echo 'lock contention changed another builder owner' >&2
  exit 1
}

printf 'http-provenance-tests=18\n'
