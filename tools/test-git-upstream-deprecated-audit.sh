#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="${ZMIN_UPSTREAM_GIT_TAG:-v2.55.0}"
cache_root="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
source_dir="$cache_root/git-${ZMIN_UPSTREAM_GIT_TAG:-v2.55.0}"

if [[ ! -d "$source_dir/t" ]]; then
  echo "missing upstream Git source tree: $source_dir" >&2
  exit 2
fi

fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/zmin-upstream-deprecated-test.XXXXXX")"
trap 'rm -rf "$fixture_root"' EXIT
fixture_root="$(cd -P "$fixture_root" && pwd -P)"

fixture_cache="$fixture_root/cache"
fixture_source="$fixture_cache/git-$tag"
mkdir -p "$fixture_cache"
cp -R "$source_dir" "$fixture_source"
cp "$cache_root/$tag.tar.gz" "$fixture_cache/$tag.tar.gz"
chmod -R u+w "$fixture_source"
mkdir -p "$fixture_source/t/trash directory.fixture/objects/deep/nested"
printf 'deprecated marker in nested stale trash\n' \
  >"$fixture_source/t/trash directory.fixture/objects/deep/nested/t9999-nested.sh"
printf 'nested sentinel\n' \
  >"$fixture_source/t/trash directory.fixture/objects/deep/nested/sentinel"
cache_root="$fixture_cache"
source_dir="$fixture_source"

audit="$repo_root/tools/git-upstream-deprecated-audit.sh"
manifest="$repo_root/tools/git-upstream-compat-manifest.sh"

ZMIN_UPSTREAM_GIT_CACHE="$cache_root" "$audit" summary >"$fixture_root/summary.tsv"
ZMIN_UPSTREAM_GIT_CACHE="$cache_root" "$audit" audit >"$fixture_root/audit.tsv"

manifest_file="$fixture_root/all-nondeprecated.tsv"
manifest_names="$fixture_root/manifest-names"
expected_names="$fixture_root/expected-names"
expected_excludes="$fixture_root/expected-excludes"
ZMIN_UPSTREAM_GIT_CACHE="$cache_root" "$manifest" all-nondeprecated >"$manifest_file"
awk -F '\t' 'NR > 1 { print $2 }' "$manifest_file" >"$manifest_names"

awk -F '\t' '
  NR == 1 || /^#/ || NF < 5 { next }
  $5 == "upstream deprecated/removed" { print "^" $1 "\\.sh$" }
' "$repo_root/tools/git-upstream-compat-tests-legacy-excludes.tsv" >"$expected_excludes"
if [[ -f "$repo_root/tools/git-upstream-compat-tests-file-excludes.tsv" ]]; then
  awk -F '\t' '
    NR == 1 || /^#/ || NF < 1 || $1 == "" { next }
    { print "^" $1 "$" }
  ' "$repo_root/tools/git-upstream-compat-tests-file-excludes.tsv" >>"$expected_excludes"
fi

find "$source_dir/t" -maxdepth 1 -type f -name 't[0-9][0-9][0-9][0-9]-*.sh' -print |
  sed 's#^.*/##' |
  LC_ALL=C sort |
  while IFS= read -r test_name; do
    if [[ -s "$expected_excludes" ]] && grep -Eq -f "$expected_excludes" <<<"$test_name"; then
      continue
    fi
    printf '%s\n' "$test_name"
  done >"$expected_names"

manifest_count="$(wc -l <"$manifest_names" | tr -d ' ')"
[[ "$manifest_count" == 1045 ]]
cmp -s "$manifest_names" "$expected_names"
if ! rg -n -i \
  'deprecated|scheduled for removal|will be removed in Git 3\.0|--i-still-use-this' \
  "$source_dir/t" -g 't[0-9][0-9][0-9][0-9]-*.sh' |
  grep -Fq '/t9999-nested.sh:'; then
  echo 'nested audit sentinel was not visible to the recursive-baseline check' >&2
  exit 1
fi
if grep -Fqx 't9999-nested.sh' "$manifest_names" ||
  awk -F '\t' '$2 == "t9999-nested.sh" { found = 1 } END { exit(found ? 0 : 1) }' \
    "$fixture_root/audit.tsv"; then
  echo 'nested stale trash/object files entered the direct test surface' >&2
  exit 1
fi
if grep -Fqx 't5323-pack-redundant.sh' "$manifest_names"; then
  echo 'fully excluded t5323 unexpectedly appears in all-nondeprecated manifest' >&2
  exit 1
fi
for retained_external in \
  t9100-git-svn-basic.sh \
  t9400-git-cvsserver-server.sh \
  t9500-gitweb-standalone-no-errors.sh \
  t9600-cvsimport.sh \
  t9800-git-p4-basic.sh; do
 grep -Fqx "$retained_external" "$manifest_names"
done

marker_backup="$fixture_root/source-marker.backup"
cp "$source_dir/.zmin-pristine-source.sha256" "$marker_backup"
printf 'forged-source-marker\n' >"$source_dir/.zmin-pristine-source.sha256"
if ZMIN_UPSTREAM_GIT_CACHE="$cache_root" "$audit" source-files \
  >"$fixture_root/invalid-source.out" 2>"$fixture_root/invalid-source.err"; then
  echo 'invalid source marker was accepted' >&2
  exit 1
fi
grep -Fqx "pinned upstream source marker mismatch: $source_dir" \
  "$fixture_root/invalid-source.err"
mv "$marker_backup" "$source_dir/.zmin-pristine-source.sha256"

expect_audit_fields() {
  local classification="$1" test_name="$2" deprecated_markers="$3" breaking_change_markers="$4"
  awk -F '\t' \
    -v classification="$classification" \
    -v test_name="$test_name" \
    -v deprecated_markers="$deprecated_markers" \
    -v breaking_change_markers="$breaking_change_markers" '
      NR > 1 &&
        $1 == classification &&
        $2 == test_name &&
        $3 == deprecated_markers &&
        $4 == breaking_change_markers { matches += 1 }
      END { exit(matches == 1 ? 0 : 1) }
    ' "$fixture_root/audit.tsv"
}

grep -Fqx $'classification\ttest\tdeprecated_markers\tbreaking_change_markers\treason' "$fixture_root/audit.tsv"
grep -Fqx $'marker_top_level_shell_files\t20\ttop-level upstream shell tests with either deprecated/removal or WITH_BREAKING_CHANGES markers' "$fixture_root/summary.tsv"
grep -Fqx $'deprecated_top_level_shell_files\t14\ttop-level upstream shell tests with explicit deprecated/removal markers; WITH_BREAKING_CHANGES is excluded' "$fixture_root/summary.tsv"
grep -Fqx $'breaking_change_marker_files\t13\ttop-level upstream shell tests containing WITH_BREAKING_CHANGES' "$fixture_root/summary.tsv"
[[ "$manifest_count" == 1045 ]]

expect_audit_fields breaking-change-in-scope t0001-init.sh 0 1
expect_audit_fields breaking-change-in-scope t0035-safe-bare-repository.sh 0 2
expect_audit_fields breaking-change-in-scope t5505-remote.sh 0 3
expect_audit_fields breaking-change-in-scope t5516-fetch-push.sh 0 4

expect_audit_fields fully-excluded-family t5323-pack-redundant.sh 2 1

printf 'deprecated audit marker split passed; denominator=%s/1046\n' "$manifest_count"
