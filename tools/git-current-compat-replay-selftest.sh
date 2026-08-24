#!/usr/bin/env bash
set -euo pipefail

# Offline parser contract. Every case invokes the production helper CLI; this
# script does not duplicate log parsing or outcome classification.
proposal_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
helper="$proposal_root/tools/git-current-compat-replay.sh"
fixture="$proposal_root/tools/git-current-compat-fixtures/t9800-git-p4-skip.log"
test -x "$helper" -o -f "$helper"
test -f "$fixture"

python3 - "$proposal_root/tools/git-current-compat-contract.json" <<'PY'
import json
import sys


def unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle, object_pairs_hook=unique_pairs)
assert set(contract) == {"contract_version", "upstream", "manifest"}
assert contract["contract_version"] == 1
assert set(contract["upstream"]) == {"tag", "archive_url", "archive_sha256", "tag_object", "commit"}
assert set(contract["manifest"]) == {"mode", "top_level_count", "selected_count", "sole_exclusion", "top_level_names_sha256", "selected_names_sha256", "generated_tsv_sha256", "names_encoding"}
assert contract["upstream"]["tag"] == "v2.55.0"
assert contract["upstream"]["commit"] == "e9019fcafe0040228b8631c30f97ae1adb61bcdc"
assert contract["manifest"]["top_level_count"] == 1046
assert contract["manifest"]["selected_count"] == 1045
assert contract["manifest"]["sole_exclusion"] == "t5323-pack-redundant.sh"
PY

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/git-current-compat-selftest.XXXXXX")"
cleanup() {
  chmod -R u+w "$tmp_root" 2>/dev/null || true
  rm -rf -- "$tmp_root"
}
trap cleanup EXIT

make_manifest() {
  local path="$1"
  local test_name="$2"
  printf '# mode\ttest\treason\nfixture\t%s\toffline self-test\n' "$test_name" >"$path"
}

run_case() {
  local case_name="$1"
  local expected_rc="$2"
  local expected_reason="$3"
  local expected_optional_rows="$4"
  local expected_assertion_count="$5"
  local expected_top_level_count="$6"
  local manifest="$tmp_root/$case_name/manifest.tsv"
  local logs="$tmp_root/$case_name/logs"
  local out="$tmp_root/$case_name/out"
  mkdir -p "$logs"
  make_manifest "$manifest" "$7"

  local rc
  if bash "$helper" --selftest-parse-lane control "$manifest" "$logs" "$out"; then
    rc=0
  else
    rc=$?
  fi
  test "$rc" -eq "$expected_rc"
  test "$(awk -F '\t' '$1 == "exit_code" { print $2; exit }' "$out/outcome.tsv")" -eq "$expected_rc"
  test "$(awk -F '\t' '$1 == "reason" { print $2; exit }' "$out/outcome.tsv")" = "$expected_reason"
  test "$(awk -F '\t' 'NR > 1 { count += 1 } END { print count + 0 }' "$out/optional-skips.tsv")" -eq "$expected_optional_rows"
  test "$(awk -F '\t' '$1 == "top_level_skip_count" { print $2; exit }' "$out/outcome.tsv")" -eq "$expected_top_level_count"
  if [[ "$expected_assertion_count" == numeric:* ]]; then
    expected_numeric="${expected_assertion_count#numeric:}"
    test "$(awk -F '\t' 'NR > 1 { total += $3 } END { print total + 0 }' "$out/assertion-skips.tsv")" -eq "$expected_numeric"
  fi
  test -s "$out/checksums.sha256"
  printf '%s: pass (exit %s, reason %s)\n' "$case_name" "$rc" "$expected_reason"
}

mkdir -p "$tmp_root/normal/logs"
make_manifest "$tmp_root/normal/manifest.tsv" t0000-normal.sh
printf 'ok 1 - normal\n1..1\n' >"$tmp_root/normal/logs/t0000-normal.log"
run_case normal 0 pass 0 numeric:0 0 t0000-normal.sh

mkdir -p "$tmp_root/git-p4/logs"
make_manifest "$tmp_root/git-p4/manifest.tsv" t9800-git-p4.sh
cp "$fixture" "$tmp_root/git-p4/logs/t9800-git-p4.log"
run_case git-p4 1 incomplete-optional-skips 1 numeric:1 1 t9800-git-p4.sh
test "$(awk -F '\t' 'NR > 1 { print $3; exit }' "$tmp_root/git-p4/out/optional-skips.tsv")" = 'top-level TAP 1..0 # SKIP p4 prerequisite unavailable'

mkdir -p "$tmp_root/missing/logs"
make_manifest "$tmp_root/missing/manifest.tsv" t0001-missing.sh
run_case missing 1 invalid-missing-logs 1 0 0 t0001-missing.sh

mkdir -p "$tmp_root/assertion-only/logs"
make_manifest "$tmp_root/assertion-only/manifest.tsv" t0002-assertion-only.sh
printf 'ok 1 - platform detail # SKIP unavailable platform assertion\n1..1\n' \
  >"$tmp_root/assertion-only/logs/t0002-assertion-only.log"
run_case assertion-only 0 pass 0 numeric:1 0 t0002-assertion-only.sh

authority_archive="${ZMIN_CURRENT_GIT_AUTHORITY_ARCHIVE:-}"
compat_repo_root="${ZMIN_COMPAT_REPO_ROOT:-$proposal_root}"
test -n "$authority_archive"
test -f "$authority_archive"
test -x "$compat_repo_root/tools/git-upstream-compat-manifest.sh"
test -x "$compat_repo_root/tools/git-upstream-deprecated-audit.sh"
staged_authority_archive="$tmp_root/authority/v2.55.0.tar.gz"
mkdir -p "$(dirname "$staged_authority_archive")"
cp -- "$authority_archive" "$staged_authority_archive"
prep_cache="$tmp_root/prep-cache"
prep_out="$tmp_root/prep-out"
if bash "$helper" --selftest-prepare-manifest-cache \
  "$staged_authority_archive" "$prep_cache" "$compat_repo_root" "$prep_out"; then
  prep_rc=0
else
  prep_rc=$?
fi
test "$prep_rc" -eq 0
test "$(awk -F '\t' '$1 == "top_level_count" { print $2; exit }' "$prep_out/selftest-summary.tsv")" -eq 1046
test "$(awk -F '\t' '$1 == "selected_count" { print $2; exit }' "$prep_out/selftest-summary.tsv")" -eq 1045
test "$(awk -F '\t' '$1 == "sole_exclusion" { print $2; exit }' "$prep_out/selftest-summary.tsv")" = t5323-pack-redundant.sh
test -f "$prep_cache/v2.55.0.tar.gz" && test ! -L "$prep_cache/v2.55.0.tar.gz"
test -f "$prep_cache/git-v2.55.0/.zmin-pristine-source.sha256"
test "$(cat "$prep_cache/git-v2.55.0/.zmin-pristine-source.sha256")" = 72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49
python3 - "$prep_cache/git-v2.55.0/.zmin-pristine-source.sha256" <<'PY'
import os
import stat
import sys

with open(sys.argv[1], "rb") as handle:
    assert handle.read() == b"72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49\n"
assert stat.S_IMODE(os.stat(sys.argv[1]).st_mode) == 0o444
PY
printf 'cached manifest/deprecated audit self-test: pass (1045/1046, only t5323 excluded)\n'
printf 'production parse self-test: all cases passed\n'
