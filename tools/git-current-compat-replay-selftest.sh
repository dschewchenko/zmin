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
assert set(contract) == {"contract_version", "upstream", "manifest", "skip_policy"}
assert contract["contract_version"] == 2
assert set(contract["upstream"]) == {"tag", "archive_url", "archive_sha256", "tag_object", "commit"}
assert set(contract["manifest"]) == {"mode", "top_level_count", "selected_count", "sole_exclusion", "top_level_names_sha256", "selected_names_sha256", "generated_tsv_sha256", "names_encoding"}
assert contract["upstream"]["tag"] == "v2.55.0"
assert contract["upstream"]["commit"] == "e9019fcafe0040228b8631c30f97ae1adb61bcdc"
assert contract["manifest"]["top_level_count"] == 1046
assert contract["manifest"]["selected_count"] == 1045
assert contract["manifest"]["sole_exclusion"] == "t5323-pack-redundant.sh"
assert contract["skip_policy"]["version"] == 1
assert contract["skip_policy"]["base_profile"] == "linux-ubuntu-24.04-x86_64"
assert [entry["test"] for entry in contract["skip_policy"]["entries"]] == sorted(entry["test"] for entry in contract["skip_policy"]["entries"])
assert len(contract["skip_policy"]["entries"]) == 7
PY

python3 - "$proposal_root/.github/workflows/git-current-compat.yml" <<'PY'
import sys

workflow_path = sys.argv[1]
lines = open(workflow_path, encoding="utf-8").read().splitlines()


def first_index(predicate):
    for index, line in enumerate(lines):
        if predicate(line):
            return index
    raise AssertionError("workflow assertion did not match")


p4_download = first_index(lambda line: "download_verified p4" in line)
p4d_download = first_index(lambda line: "download_verified p4d" in line)
jgit_download = first_index(lambda line: "download_verified jgit" in line)
path_export = first_index(lambda line: 'export PATH="$toolbin:' in line)
required_loop = first_index(lambda line: "for command in awk bash cat comm curl" in line)
jgit_canonical = first_index(lambda line: "canonical_required_executable ZMIN_UPSTREAM_CONTRACT_JGIT jgit" in line)
owned_verify = first_index(lambda line: "verify_provisioned_tool p4" in line)
owned_version = first_index(lambda line: 'jgit_version="$(jgit --version' in line)
github_env_write = first_index(lambda line: '>>"$GITHUB_ENV"' in line)
completion = first_index(lambda line: "dependency_preflight_complete" in line)
replay_success = first_index(lambda line: line.strip() == "if: success()")
setup_start = first_index(lambda line: line.strip() == "- name: Install pinned Git test dependencies")
setup_end = first_index(lambda line: line.strip() == "- name: Run stock control and Zmin replay")
setup_text = "\n".join(lines[setup_start:setup_end])
ambient_record = first_index(lambda line: "ambient_command\\t%s\\t%s\\t%s" in line)
ambient_loop = first_index(lambda line: "for command in apt-cache apt-get awk curl" in line)

assert p4_download < p4d_download < jgit_download < path_export < owned_verify < owned_version
assert owned_version < github_env_write
assert owned_version < required_loop < jgit_canonical < completion
assert "set -Eeuo pipefail" in setup_text
assert "exit 1" not in setup_text
assert "return 1" in setup_text
assert ambient_record < ambient_loop
assert "test \"$canonical\" = \"$path\"" not in setup_text
assert 'replay_stage="ambient-command-canonicalization:$command"' in setup_text
workflow_text = "\n".join(lines)
assert "github.event.before == '45108021d7883ec8011b4d03bdd0aec0606851b7'" in workflow_text
assert "github.event.before == '9ac8723b671d845cb6181b6b0b88e6bb93a97531'" not in workflow_text
assert "github.event.before == '18a0f0d455337385e1b6a25312fb15879cdf5145'" not in workflow_text
assert replay_success > completion
assert "if: always()" not in "\n".join(lines[replay_success - 3: replay_success + 2])
print("workflow provisioning order: pass (owned downloads, PATH, verification, versions precede generic lookup/canonicalization; replay is success-gated)")
PY

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/git-current-compat-selftest.XXXXXX")"
cleanup() {
  chmod -R u+w "$tmp_root" 2>/dev/null || true
  rm -rf -- "$tmp_root"
}
trap cleanup EXIT

marker_probe="$tmp_root/missing-preflight"
mkdir -p "$marker_probe/artifacts"
if REPLAY_PROFILE=linux-ubuntu-24.04-x86_64 \
  REPLAY_RUST_TOOLCHAIN=1.98.0-x86_64-unknown-linux-gnu \
  ZMIN_REPLAY_RUSTUP=/bin/true \
  bash "$helper" 1 0 "$marker_probe/artifacts" >/dev/null 2>&1; then
  echo 'replay helper accepted missing dependency preflight marker' >&2
  exit 1
fi
test "$(awk -F '\t' '$1 == "reason" { print $2; exit }' "$marker_probe/artifacts/outcome.tsv")" = \
  'dependency preflight is missing; refusing to run without provisioned tools'
printf '%s\n' 'dependency preflight marker guard: pass (missing marker rejected before replay)'

dynamic_failure_script="$tmp_root/dynamic-setup-failure.sh"
dynamic_success_script="$tmp_root/dynamic-setup-success.sh"
dynamic_failure_file="$tmp_root/dynamic-setup-failure.tsv"
python3 - "$proposal_root/.github/workflows/git-current-compat.yml" \
  "$dynamic_failure_script" "$dynamic_success_script" <<'PY'
import sys
import textwrap

workflow_path, failure_path, success_path = sys.argv[1:]
lines = open(workflow_path, encoding="utf-8").read().splitlines()
start = next(i for i, line in enumerate(lines) if line.strip() == "record_setup_failure() {")
end = next(i for i, line in enumerate(lines[start:], start) if line.strip() == "trap record_setup_failure ERR")
function_block = textwrap.dedent("\n".join(lines[start:end]))
prefix = textwrap.dedent(
    """\
    #!/usr/bin/env bash
    set -Eeuo pipefail
    dependency_preflight_failure="$1"
    replay_stage=dynamic-controlled-failure
    """
)
failure_script = prefix + function_block + textwrap.dedent(
    """
    trap record_setup_failure ERR
    controlled_failure() {
      false
    }
    controlled_failure
    """
)
success_script = prefix.replace("dynamic-controlled-failure", "dynamic-controlled-success") + function_block + textwrap.dedent(
    """
    trap record_setup_failure ERR
    controlled_success() {
      :
    }
    controlled_success
    test ! -e "$dependency_preflight_failure"
    """
)
open(failure_path, "w", encoding="utf-8").write(failure_script)
open(success_path, "w", encoding="utf-8").write(success_script)
PY
chmod 755 "$dynamic_failure_script" "$dynamic_success_script"
if env DYNAMIC_SECRET=dynamic-secret-value bash "$dynamic_failure_script" "$dynamic_failure_file"; then
  echo 'ERR trap dynamic failure unexpectedly succeeded' >&2
  exit 1
fi
test "$(awk -F '\t' '$1 == "result" { print $2; exit }' "$dynamic_failure_file")" = fail
test "$(grep -Fc $'result\tfail' "$dynamic_failure_file")" -eq 1
test "$(awk -F '\t' '$1 == "stage" { print $2; exit }' "$dynamic_failure_file")" = dynamic-controlled-failure
dynamic_line="$(awk -F '\t' '$1 == "line" { print $2; exit }' "$dynamic_failure_file")"
test "$dynamic_line" -gt 0
dynamic_command="$(awk -F '\t' '$1 == "command" { print $2; exit }' "$dynamic_failure_file")"
test -n "$dynamic_command"
test "$dynamic_command" = false
test "$(awk -F '\t' '$1 == "exit_code" { print $2; exit }' "$dynamic_failure_file")" -eq 1
if grep -Fq 'dynamic-secret-value' "$dynamic_failure_file"; then
  echo 'ERR trap artifact leaked environment secret' >&2
  exit 1
fi
bash "$dynamic_success_script" "$tmp_root/dynamic-setup-success.tsv"
printf '%s\n' 'ERR trap dynamic guard: pass (phase/line/quoted command/exit captured once; success stays clean)'

make_manifest() {
  local path="$1"
  local test_name="$2"
  printf '# mode\ttest\treason\nfixture\t%s\toffline self-test\n' "$test_name" >"$path"
}

policy_file="$tmp_root/skip-policy.tsv"
cp "$proposal_root/tools/git-current-compat-fixtures/skip-policy.tsv" "$policy_file"
python3 - "$proposal_root/tools/git-current-compat-contract.json" "$policy_file" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as handle:
    entries = json.load(handle)["skip_policy"]["entries"]
with open(sys.argv[2], encoding="utf-8") as handle:
    fixture = [line.rstrip("\n").split("\t") for line in handle]
assert fixture[0] == ["test", "classification", "required_run_profile", "reason"]
assert fixture[1:] == [[entry["test"], entry["classification"], entry["required_run_profile"], entry["reason"]] for entry in entries]
PY

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
  if bash "$helper" --selftest-parse-lane control "$manifest" "$logs" "$out" "$policy_file"; then
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
test "$(awk -F '\t' 'NR > 1 { print $5; exit }' "$tmp_root/git-p4/out/optional-skips.tsv")" = 'p4 prerequisite unavailable'

mkdir -p "$tmp_root/missing/logs"
make_manifest "$tmp_root/missing/manifest.tsv" t0001-missing.sh
run_case missing 1 invalid-missing-logs 1 0 0 t0001-missing.sh

mkdir -p "$tmp_root/assertion-only/logs"
make_manifest "$tmp_root/assertion-only/manifest.tsv" t0002-assertion-only.sh
printf 'ok 1 - platform detail # SKIP unavailable platform assertion\n1..1\n' \
  >"$tmp_root/assertion-only/logs/t0002-assertion-only.log"
run_case assertion-only 0 pass 0 numeric:1 0 t0002-assertion-only.sh

canonical_fixture="$tmp_root/canonical-executable"
mkdir -p "$canonical_fixture"
canonical_real="$canonical_fixture/real-tool"
canonical_link="$canonical_fixture/tool-link"
canonical_dangling="$canonical_fixture/dangling-link"
canonical_nonexec="$canonical_fixture/nonexec"
printf '#!/bin/sh\nexit 0\n' >"$canonical_real"
chmod 755 "$canonical_real"
ln -s "$(basename "$canonical_real")" "$canonical_link"
ln -s missing-target "$canonical_dangling"
printf 'not executable\n' >"$canonical_nonexec"
canonical_resolved="$canonical_real"
if command -v realpath >/dev/null 2>&1 && realpath -e "$canonical_real" >/dev/null 2>&1; then
  canonical_resolved="$(realpath -e -- "$canonical_real")"
  test "$(bash "$helper" --selftest-canonical-executable "$canonical_link")" = "$canonical_resolved"
  if bash "$helper" --selftest-canonical-executable "$canonical_dangling" >/dev/null 2>&1; then
    echo 'dangling executable symlink was accepted' >&2
    exit 1
  fi
  if bash "$helper" --selftest-canonical-executable "$canonical_nonexec" >/dev/null 2>&1; then
    echo 'non-executable regular file was accepted' >&2
    exit 1
  fi
  printf 'canonical executable fixture: pass (symlink resolved; dangling/non-executable rejected)\n'
else
  canonical_resolved="$(realpath "$canonical_real")"
  python3 - "$canonical_link" "$canonical_real" "$canonical_dangling" "$canonical_nonexec" <<'PY'
import os
import stat
import sys

link, real, dangling, nonexec = sys.argv[1:]
assert os.path.realpath(link) == os.path.realpath(real)
assert stat.S_ISREG(os.stat(real).st_mode) and os.access(real, os.X_OK)
assert not os.path.exists(dangling)
assert not os.access(nonexec, os.X_OK)
PY
  printf 'canonical executable fixture: pass (portable equivalent; realpath unavailable)\n'
fi
if env -u ZMIN_REPLAY_RUSTUP bash "$helper" --selftest-rustup-binding >/dev/null 2>&1; then
  echo 'unset ZMIN_REPLAY_RUSTUP was accepted' >&2
  exit 1
fi
if ZMIN_REPLAY_RUSTUP=relative-rustup bash "$helper" --selftest-rustup-binding >/dev/null 2>&1; then
  echo 'relative ZMIN_REPLAY_RUSTUP was accepted' >&2
  exit 1
fi
if ZMIN_REPLAY_RUSTUP="$canonical_nonexec" bash "$helper" --selftest-rustup-binding >/dev/null 2>&1; then
  echo 'non-executable ZMIN_REPLAY_RUSTUP was accepted' >&2
  exit 1
fi
if ZMIN_REPLAY_RUSTUP="$canonical_link" bash "$helper" --selftest-rustup-binding >/dev/null 2>&1; then
  echo 'symlink ZMIN_REPLAY_RUSTUP was accepted' >&2
  exit 1
fi
test "$(ZMIN_REPLAY_RUSTUP="$canonical_resolved" bash "$helper" --selftest-rustup-binding)" = "$canonical_resolved"
printf 'rustup binding fixture: pass (unset/relative/non-executable/symlink rejected; canonical path accepted)\n'

if realpath -e "$canonical_real" >/dev/null 2>&1; then
ambient_fixture="$tmp_root/ambient-alternatives"
ambient_bin="$ambient_fixture/bin"
ambient_real="$ambient_fixture/ambient-real"
ambient_link="$ambient_bin/ambient-tool"
ambient_dangling="$ambient_bin/ambient-dangling"
ambient_nonexec="$ambient_bin/ambient-nonexec"
ambient_preflight="$tmp_root/.replay/dependency-preflight.tsv"
ambient_script="$tmp_root/ambient-canonicalization.sh"
mkdir -p "$ambient_bin" "$tmp_root/.replay"
printf '#!/bin/sh\nexit 0\n' >"$ambient_real"
chmod 755 "$ambient_real"
ln -s "$ambient_real" "$ambient_link"
ln -s missing-target "$ambient_dangling"
printf 'not executable\n' >"$ambient_nonexec"
python3 - "$proposal_root/.github/workflows/git-current-compat.yml" "$ambient_script" <<'PY'
import sys
import textwrap

workflow_path, script_path = sys.argv[1:]
lines = open(workflow_path, encoding="utf-8").read().splitlines()
path_start = next(i for i, line in enumerate(lines) if line.strip() == "path_is_safe() {")
loop_start = next(i for i, line in enumerate(lines[path_start:], path_start) if line.strip().startswith("for command in apt-cache apt-get awk curl"))
function_block = textwrap.dedent("\n".join(lines[path_start:loop_start]))
script = textwrap.dedent(
    """\
    #!/usr/bin/env bash
    set -Eeuo pipefail
    """
) + function_block + textwrap.dedent(
    """
    assert_ambient_canonical "$1"
    """
)
open(script_path, "w", encoding="utf-8").write(script)
PY
chmod 755 "$ambient_script"
PATH="$ambient_bin:$PATH" GITHUB_WORKSPACE="$tmp_root" bash "$ambient_script" ambient-tool
ambient_record_line="$(awk -F '\t' '$1 == "ambient_command" { print; exit }' "$ambient_preflight")"
ambient_canonical="$(realpath -e -- "$ambient_real")"
expected_ambient_record=$'ambient_command\tambient-tool\t'"$ambient_link"$'\t'"$ambient_canonical"
test "$ambient_record_line" = "$expected_ambient_record"
if PATH="$ambient_bin:$PATH" GITHUB_WORKSPACE="$tmp_root" bash "$ambient_script" ambient-dangling >/dev/null 2>&1; then
  echo 'dangling ambient alternative was accepted' >&2
  exit 1
fi
if PATH="$ambient_bin:$PATH" GITHUB_WORKSPACE="$tmp_root" bash "$ambient_script" ambient-nonexec >/dev/null 2>&1; then
  echo 'non-executable ambient alternative was accepted' >&2
  exit 1
fi
if ZMIN_REPLAY_RUSTUP="$canonical_link" bash "$helper" --selftest-rustup-binding >/dev/null 2>&1; then
  echo 'final-bound symlink was accepted' >&2
  exit 1
fi
printf '%s\n' 'ambient alternatives fixture: pass (symlink spelling recorded with canonical target; dangling/nonexec rejected; final bound symlink rejected)'
else
printf '%s\n' 'ambient alternatives fixture: skipped (realpath -e unavailable on this host; production Ubuntu path remains statically checked)'
fi

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
aggregate_root="$tmp_root/aggregate"
aggregate_contract_sha="$(shasum -a 256 "$proposal_root/tools/git-current-compat-contract.json" | awk '{print $1}')"
aggregate_manifest_sha="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["manifest"]["generated_tsv_sha256"])' "$proposal_root/tools/git-current-compat-contract.json")"
make_profile_artifact() {
  local profile="$1"
  local root="$aggregate_root/$profile"
  mkdir -p "$root/scope" "$root/control" "$root/zmin"
  cp "$prep_out/all-nondeprecated.tsv" "$root/scope/selected-manifest.tsv"
  cat >"$root/metadata.tsv" <<EOF
workflow_commit	e05616c443cfeab060be12c23414ec6d307b4d21
scope	full1045
profile	$profile
expected_tests	1045
jobs	4
per_test_timeout_seconds	0
classification	authoritative
authority_rule	full1045 with per_test_timeout=0 only
upstream_tag	v2.55.0
upstream_commit	e9019fcafe0040228b8631c30f97ae1adb61bcdc
upstream_tag_object	5ce91c059e41090e7d2cffad39c04af8acf98dc1
upstream_archive_sha256	72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49
no_retries_or_rerolls	true
repository_writes	none; contents:read only
secrets	none
runner	fixture
contract_sha256	$aggregate_contract_sha
manifest_sha256	$aggregate_manifest_sha
EOF
  for lane in control zmin; do
    printf 'mode\ttest\tstatus\treason\tlog\n' >"$root/$lane/summary.tsv"
    while IFS=$'\t' read -r _ test_name _; do
      [[ -n "$test_name" ]] || continue
      log="$root/$lane/${test_name%.sh}.log"
      printf 'ok 1 - fixture\n1..1\n' >"$log"
      printf 'all-nondeprecated\t%s\tpass\t\t%s\n' "$test_name" "${test_name%.sh}.log" >>"$root/$lane/summary.tsv"
    done < <(tail -n +2 "$prep_out/all-nondeprecated.tsv")
    cat >"$root/$lane/status.tsv" <<EOF
role	$lane
stock_control	$([[ "$lane" == control ]] && printf 1 || printf 0)
command_exit	0
summary_present	true
summary_rows	1045
summary_passes	1045
summary_failures	0
expected_rows	1045
EOF
  done
  printf 'lane\ttest\tclassification\trequired_run_profile\treason\tlog_sha256\n' >"$root/optional-skips.tsv"
  python3 - "$root" <<'PY'
import hashlib
import os
import pathlib
import sys
root = pathlib.Path(sys.argv[1])
rows = []
for path in sorted(root.rglob("*")):
    if path.name == "checksums.sha256" or not path.is_file() or path.is_symlink():
        continue
    rows.append(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(root).as_posix()}")
(root / "checksums.sha256").write_text("\n".join(rows) + "\n")
PY
}
refresh_checksums() {
  python3 - "$1" <<'PY'
import hashlib
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
rows = []
for path in sorted(root.rglob("*")):
    if path.name == "checksums.sha256" or not path.is_file() or path.is_symlink():
        continue
    rows.append(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(root).as_posix()}")
(root / "checksums.sha256").write_text("\n".join(rows) + "\n")
PY
}
make_profile_artifact linux-ubuntu-24.04-x86_64
if python3 "$proposal_root/tools/git-current-compat-aggregate.py" \
  --contract "$proposal_root/tools/git-current-compat-contract.json" \
  --profile linux-ubuntu-24.04-x86_64="$aggregate_root/linux-ubuntu-24.04-x86_64" \
  >"$tmp_root/aggregate-pending.tsv"; then
  echo 'Linux-only aggregate was incorrectly accepted as cross-platform pass' >&2
  exit 1
fi
test "$(awk -F '\t' '$1 == "cross_platform_status" { print $2; exit }' "$tmp_root/aggregate-pending.tsv")" = pending
test "$(awk -F '\t' '$1 == "closure_gap_count" { print $2; exit }' "$tmp_root/aggregate-pending.tsv")" = 7
for profile in windows-x86_64 macos-native linux-case-insensitive-fs linux-privileged-root linux-high-disk; do
  make_profile_artifact "$profile"
done
aggregate_args=()
for profile in linux-ubuntu-24.04-x86_64 windows-x86_64 macos-native linux-case-insensitive-fs linux-privileged-root linux-high-disk; do
  aggregate_args+=(--profile "$profile=$aggregate_root/$profile")
done
python3 "$proposal_root/tools/git-current-compat-aggregate.py" \
  --contract "$proposal_root/tools/git-current-compat-contract.json" "${aggregate_args[@]}" \
  >"$tmp_root/aggregate-pass.tsv"
test "$(awk -F '\t' '$1 == "cross_platform_status" { print $2; exit }' "$tmp_root/aggregate-pass.tsv")" = pass
expect_aggregate_failure() {
  if python3 "$proposal_root/tools/git-current-compat-aggregate.py" \
    --contract "$proposal_root/tools/git-current-compat-contract.json" \
    --profile linux-ubuntu-24.04-x86_64="$base_artifact" >/dev/null 2>&1; then
    echo "aggregate accepted adversarial artifact" >&2
    exit 1
  fi
}
base_artifact="$aggregate_root/linux-ubuntu-24.04-x86_64"
base_metadata_backup="$tmp_root/metadata.backup"
cp "$base_artifact/metadata.tsv" "$base_metadata_backup"
printf 'profile\tduplicate\n' >>"$base_artifact/metadata.tsv"
expect_aggregate_failure
mv "$base_metadata_backup" "$base_artifact/metadata.tsv"
first_summary="$base_artifact/control/summary.tsv"
summary_backup="$tmp_root/summary.backup"
cp "$first_summary" "$summary_backup"
sed -i '' '2s/\tpass\t/\tfail\t/' "$first_summary" 2>/dev/null || sed -i '2s/\tpass\t/\tfail\t/' "$first_summary"
expect_aggregate_failure
mv "$summary_backup" "$first_summary"
cp "$first_summary" "$summary_backup"
python3 - "$first_summary" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
rows = path.read_text().splitlines()
fields = rows[1].split("\t")
fields[-1] = "../zmin/" + fields[-1]
rows[1] = "\t".join(fields)
path.write_text("\n".join(rows) + "\n")
PY
refresh_checksums "$base_artifact"
expect_aggregate_failure
mv "$summary_backup" "$first_summary"
refresh_checksums "$base_artifact"
first_log="$(find "$base_artifact/control" -type f -name 't*.log' -print -quit)"
log_backup="$tmp_root/log.backup"
cp "$first_log" "$log_backup"
printf 'tampered\n' >>"$first_log"
expect_aggregate_failure
mv "$log_backup" "$first_log"
optional_backup="$tmp_root/optional.backup"
cp "$base_artifact/optional-skips.tsv" "$optional_backup"
printf 'control\tt0000-extra.sh\tplatform\twindows-x86_64\tbad\t%064d\n' 0 >>"$base_artifact/optional-skips.tsv"
expect_aggregate_failure
mv "$optional_backup" "$base_artifact/optional-skips.tsv"
cp "$base_artifact/optional-skips.tsv" "$optional_backup"
printf 'control\t../t0029-core-unsetenvvars.sh\tplatform\twindows-x86_64\tskipping Windows-specific tests\t%064d\n' 0 >>"$base_artifact/optional-skips.tsv"
refresh_checksums "$base_artifact"
expect_aggregate_failure
mv "$optional_backup" "$base_artifact/optional-skips.tsv"
refresh_checksums "$base_artifact"
ln -s "$base_artifact" "$tmp_root/symlink-profile"
if python3 "$proposal_root/tools/git-current-compat-aggregate.py" \
  --contract "$proposal_root/tools/git-current-compat-contract.json" \
  --profile linux-ubuntu-24.04-x86_64="$tmp_root/symlink-profile" >/dev/null 2>&1; then
  echo 'aggregate accepted symlink profile root' >&2
  exit 1
fi
rm "$tmp_root/symlink-profile"
duplicate_contract="$tmp_root/duplicate-contract.json"
python3 - "$proposal_root/tools/git-current-compat-contract.json" "$duplicate_contract" <<'PY'
import pathlib
import sys
text = pathlib.Path(sys.argv[1]).read_text()
pathlib.Path(sys.argv[2]).write_text(text.replace('"contract_version": 2,', '"contract_version": 2, "contract_version": 2,', 1))
PY
if python3 "$proposal_root/tools/git-current-compat-aggregate.py" \
  --contract "$duplicate_contract" \
  --profile linux-ubuntu-24.04-x86_64="$base_artifact" >/dev/null 2>&1; then
  echo 'aggregate accepted duplicate JSON key' >&2
  exit 1
fi
if python3 "$proposal_root/tools/git-current-compat-aggregate.py" \
  --contract "$proposal_root/tools/git-current-compat-contract.json" \
  --profile unknown="$base_artifact" >/dev/null 2>&1; then
  echo 'aggregate accepted unknown profile' >&2
  exit 1
fi
printf 'aggregate adversarial self-test: pass (metadata/status/hash/skip/symlink/JSON/profile rejects)\n'
printf 'aggregate policy self-test: pass (Linux pending; full synthetic closure pass)\n'
printf 'cached manifest/deprecated audit self-test: pass (1045/1046, only t5323 excluded)\n'
printf 'production parse self-test: all cases passed\n'
