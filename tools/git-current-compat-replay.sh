#!/usr/bin/env bash
set -euo pipefail

# One-shot evidence producer. Mutable replay state is below the supplied
# .replay directory plus one explicitly owned checkout .tmp directory; there
# are no test retries, rerolls, ref updates, or publications.
jobs="${1:-}"
test_timeout="${2:-}"
artifact_root="${3:-}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

upstream_tag=v2.55.0
upstream_commit=e9019fcafe0040228b8631c30f97ae1adb61bcdc
upstream_tag_object=5ce91c059e41090e7d2cffad39c04af8acf98dc1
upstream_archive_sha256=72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49
upstream_archive_url="https://github.com/git/git/archive/refs/tags/${upstream_tag}.tar.gz"
upstream_repo_url=https://github.com/git/git.git
expected_full_tests=1045
replay_profile="${REPLAY_PROFILE:-}"
artifact_budget_bytes=$((2 * 1024 * 1024 * 1024))
required_rust_toolchain=1.98.0-x86_64-unknown-linux-gnu
rust_toolchain="${REPLAY_RUST_TOOLCHAIN:-}"
rustup_bin="${ZMIN_REPLAY_RUSTUP:-}"

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 2
}

tap_top_level_skip_regex='^[[:space:]]*1[[:space:]]*\.\.[[:space:]]*0[[:space:]]*#[[:space:]]*[Ss][Kk][Ii][Pp]([[:space:]]|$)'
tap_skip_regex='#[[:space:]]*[Ss][Kk][Ii][Pp]([[:space:]]|$)'

# Parse one lane's selected logs. This function is intentionally independent
# of the replay/build path so the offline self-test invokes the exact parser
# used by the full run. It returns nonzero only for malformed parser inputs;
# missing logs and TAP skips are represented by its output counters.
parse_lane_logs() {
  local lane="$1"
  local manifest="$2"
  local logs_dir="$3"
  local optional_skips_file="$4"
  local assertion_skips_file="$5"
  local policy_file="${6:-}"
  local test_name
  local log
  local log_sha256
  local tap_result
  local top_level_count
  local top_level_reason
  local top_level_detail
  local policy_row
  local classification
  local required_profile
  local expected_reason
  local all_skip_count
  local assertion_count

  parse_top_level_skip_count=0
  parse_allowed_top_level_skip_count=0
  parse_unclassified_top_level_skip_count=0
  parse_reason_mismatch_count=0
  parse_profile_mismatch_count=0
  parse_missing_log_count=0
  [[ "$lane" == control || "$lane" == zmin ]] || return 2
  [[ -f "$manifest" && -d "$logs_dir" ]] || return 2
  [[ -f "$policy_file" ]] || return 2
  [[ "$(awk -F '\t' 'NR == 1 && $2 == "test" { print "valid"; exit }' "$manifest")" == valid ]] || return 2
  awk -F '\t' 'NR > 1 && (NF < 2 || $2 == "") { bad = 1 } END { exit bad + 0 }' "$manifest" || return 2

  while IFS= read -r test_name; do
    [[ -n "$test_name" ]] || continue
    log="$logs_dir/${test_name%.sh}.log"
    if [[ ! -f "$log" ]]; then
      parse_missing_log_count=$((parse_missing_log_count + 1))
      printf '%s\t%s\tmissing-log\tmissing\tmissing\tmissing\n' "$lane" "$test_name" >>"$optional_skips_file"
      printf '%s\t%s\tmissing\tmissing\n' "$lane" "$test_name" >>"$assertion_skips_file"
      continue
    fi
    log_sha256="$(sha256sum "$log" | awk '{ print $1 }')"
    tap_result="$(LC_ALL=C awk -v pattern="$tap_top_level_skip_regex" '
      match($0, pattern) {
        count += 1
        if (!found) {
          reason = substr($0, RSTART + RLENGTH)
          sub(/^[[:space:]]+/, "", reason)
          gsub(/[[:space:]]+/, " ", reason)
          found = 1
        }
      }
      END { printf "%d\t%s\n", count + 0, reason }
    ' "$log")"
    IFS=$'\t' read -r top_level_count top_level_reason <<<"$tap_result"
    all_skip_count="$(LC_ALL=C awk -v pattern="$tap_skip_regex" \
      'match($0, pattern) { count += 1 } END { print count + 0 }' "$log")"
    assertion_count=$((all_skip_count - top_level_count))
    (( assertion_count < 0 )) && assertion_count=0
    printf '%s\t%s\t%s\t%s\n' "$lane" "$test_name" "$assertion_count" "$log_sha256" >>"$assertion_skips_file"
    if (( top_level_count > 0 )); then
      parse_top_level_skip_count=$((parse_top_level_skip_count + top_level_count))
      # Keep the strict marker and append only the captured, normalized TAP reason.
      top_level_detail="$top_level_reason"
      if [[ -n "$top_level_detail" ]]; then
        top_level_reason="top-level TAP 1..0 # SKIP $top_level_detail"
      else
        top_level_reason='top-level TAP 1..0 # SKIP'
      fi
      policy_row="$(awk -F '\t' -v test="$test_name" 'NR > 1 && $1 == test { print; exit }' "$policy_file")"
      classification=unclassified
      required_profile=none
      expected_reason=unclassified
      if [[ -n "$policy_row" ]]; then
        IFS=$'\t' read -r policy_test classification required_profile expected_reason <<<"$policy_row"
        if [[ "$top_level_detail" != "$expected_reason" ]]; then
          classification=reason-mismatch
          parse_reason_mismatch_count=$((parse_reason_mismatch_count + 1))
        elif [[ "$required_profile" != "$replay_profile" ]]; then
          parse_allowed_top_level_skip_count=$((parse_allowed_top_level_skip_count + 1))
        else
          classification=profile-mismatch
          parse_profile_mismatch_count=$((parse_profile_mismatch_count + 1))
        fi
      else
        parse_unclassified_top_level_skip_count=$((parse_unclassified_top_level_skip_count + 1))
      fi
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$lane" "$test_name" "$classification" \
        "$required_profile" "$top_level_detail" "$log_sha256" >>"$optional_skips_file"
    fi
  done < <(awk -F '\t' 'NR > 1 { print $2 }' "$manifest")
}

write_parse_checksums() {
  local out_dir="$1"
  (
    cd "$out_dir"
    while IFS= read -r -d '' file; do
      sha256sum "${file#./}"
    done < <(find . -type f ! -name checksums.sha256 -print0 | LC_ALL=C sort -z)
  ) >"$out_dir/checksums.sha256"
}

fsync_file_and_directory() {
  local file_path="$1"
  local directory_path="$2"
  python3 - "$file_path" "$directory_path" <<'PY'
import os
import sys

file_fd = os.open(sys.argv[1], os.O_RDONLY)
try:
    os.fsync(file_fd)
finally:
    os.close(file_fd)
directory_fd = os.open(sys.argv[2], os.O_RDONLY)
try:
    os.fsync(directory_fd)
finally:
    os.close(directory_fd)
PY
}

prepare_manifest_cache() {
  local authority_archive="$1"
  local manifest_cache="$2"
  local archive_path="$manifest_cache/$upstream_tag.tar.gz"
  local source_path="$manifest_cache/git-$upstream_tag"
  local source_tmp="$manifest_cache/.git-$upstream_tag.tmp.$$"
  local marker_path="$source_tmp/.zmin-pristine-source.sha256"
  local observed_sha256

  [[ -f "$authority_archive" && ! -L "$authority_archive" ]] || return 2
  observed_sha256="$(sha256sum "$authority_archive" | awk '{ print $1 }')"
  [[ "$observed_sha256" == "$upstream_archive_sha256" ]] || return 2
  tar -tzf "$authority_archive" >/dev/null 2>&1 || return 2
  [[ ! -e "$manifest_cache" && ! -L "$manifest_cache" ]] || return 2
  mkdir "$manifest_cache" || return 2
  [[ ! -e "$archive_path" && ! -L "$archive_path" ]] || return 2
  mv -- "$authority_archive" "$archive_path" || return 2
  [[ -f "$archive_path" && ! -L "$archive_path" ]] || return 2
  fsync_file_and_directory "$archive_path" "$manifest_cache" || return 2
  [[ ! -e "$source_path" && ! -e "$source_tmp" ]] || return 2
  mkdir "$source_tmp" || return 2
  tar -xzf "$archive_path" --strip-components=1 -C "$source_tmp" || return 2
  [[ -d "$source_tmp/t" ]] || return 2
  [[ ! -e "$marker_path" && ! -L "$marker_path" ]] || return 2
  printf '%s\n' "$upstream_archive_sha256" >"$marker_path"
  chmod -R a-w "$source_tmp"
  [[ -f "$marker_path" && ! -L "$marker_path" ]] || return 2
  fsync_file_and_directory "$marker_path" "$source_tmp" || return 2
  python3 - "$marker_path" "$upstream_archive_sha256" <<'PY'
import os
import stat
import sys

with open(sys.argv[1], "rb") as handle:
    assert handle.read() == (sys.argv[2] + "\n").encode("ascii")
assert stat.S_IMODE(os.stat(sys.argv[1]).st_mode) == 0o444
PY
  [[ "$?" == 0 ]] || return 2
  mv -- "$source_tmp" "$source_path" || return 2
  marker_path="$source_path/.zmin-pristine-source.sha256"
  fsync_file_and_directory "$marker_path" "$manifest_cache" || return 2
  python3 - "$marker_path" "$upstream_archive_sha256" <<'PY'
import os
import stat
import sys

with open(sys.argv[1], "rb") as handle:
    assert handle.read() == (sys.argv[2] + "\n").encode("ascii")
assert stat.S_IMODE(os.stat(sys.argv[1]).st_mode) == 0o444
PY
  [[ "$?" == 0 ]] || return 2
  [[ -d "$source_path/t" && -f "$source_path/.zmin-pristine-source.sha256" ]] || return 2
  return 0
}

path_text_is_safe() {
  local value="$1"
  [[ -n "$value" && "$value" == /* && "$value" != *$'\n'* && "$value" != *$'\r'* ]] || return 1
  python3 -c 'import sys; value=sys.argv[1]; value.encode("utf-8").decode("utf-8"); assert all((ord(ch) >= 32 and ord(ch) != 127) or ch == "\t" for ch in value)' "$value"
}

resolve_canonical_executable() {
  local path="$1"
  local canonical canonical_again
  path_text_is_safe "$path" || return 1
  if realpath -e -- "$path" >/dev/null 2>&1; then
    canonical="$(realpath -e -- "$path" 2>/dev/null)" || return 1
  else
    canonical="$(realpath "$path" 2>/dev/null)" || return 1
  fi
  path_text_is_safe "$canonical" || return 1
  [[ -f "$canonical" && -x "$canonical" && ! -L "$canonical" ]] || return 1
  if realpath -e -- "$canonical" >/dev/null 2>&1; then
    canonical_again="$(realpath -e -- "$canonical" 2>/dev/null)" || return 1
  else
    canonical_again="$(realpath "$canonical" 2>/dev/null)" || return 1
  fi
  [[ "$canonical_again" == "$canonical" ]] || return 1
  printf '%s\n' "$canonical"
}

if [[ "${1:-}" == --selftest-canonical-executable ]]; then
  [[ "$#" == 2 ]] || die 'usage: --selftest-canonical-executable <path>'
  resolve_canonical_executable "$2"
  exit $?
fi

if [[ "${1:-}" == --selftest-rustup-binding ]]; then
  [[ "$#" == 1 ]] || die 'usage: --selftest-rustup-binding'
  [[ -n "${ZMIN_REPLAY_RUSTUP:-}" ]] || die 'ZMIN_REPLAY_RUSTUP is unset'
  bound_rustup="$ZMIN_REPLAY_RUSTUP"
  path_text_is_safe "$bound_rustup" || die 'ZMIN_REPLAY_RUSTUP is not a safe absolute path'
  canonical_rustup="$(resolve_canonical_executable "$bound_rustup" || true)"
  [[ "$canonical_rustup" == "$bound_rustup" ]] || die 'ZMIN_REPLAY_RUSTUP is not canonical'
  printf '%s\n' "$canonical_rustup"
  exit 0
fi

if [[ "${1:-}" == --selftest-prepare-manifest-cache ]]; then
  [[ "$#" == 5 ]] || die 'usage: --selftest-prepare-manifest-cache <authority-archive> <manifest-cache> <tools-root> <out-dir>'
  prep_authority_archive="$2"
  prep_manifest_cache="$3"
  prep_tools_root="$4"
  prep_out_dir="$5"
  [[ -f "$prep_authority_archive" && -d "$prep_tools_root/tools" ]] || die 'cached authority archive or compatibility tools root is missing'
  [[ -x "$prep_tools_root/tools/git-upstream-compat-manifest.sh" ]] || die 'manifest helper is missing'
  [[ -x "$prep_tools_root/tools/git-upstream-deprecated-audit.sh" ]] || die 'deprecated audit helper is missing'
  [[ "$prep_manifest_cache" != "$prep_out_dir" ]] || die 'manifest cache and self-test output must be separate'
  if [[ -e "$prep_out_dir" ]] && find "$prep_out_dir" -mindepth 1 -print -quit 2>/dev/null | grep -q .; then
    die 'self-test output directory is not empty'
  fi
  mkdir -p "$prep_out_dir"
  prepare_manifest_cache "$prep_authority_archive" "$prep_manifest_cache" || die 'manifest cache preparation failed'
  prep_archive="$prep_manifest_cache/$upstream_tag.tar.gz"
  prep_source="$prep_manifest_cache/git-$upstream_tag"
  test -f "$prep_archive" && test ! -L "$prep_archive"
  test -f "$prep_source/.zmin-pristine-source.sha256" && test ! -L "$prep_source/.zmin-pristine-source.sha256"
  test "$(cat "$prep_source/.zmin-pristine-source.sha256")" = "$upstream_archive_sha256"
  prep_manifest="$prep_out_dir/all-nondeprecated.tsv"
  prep_audit="$prep_out_dir/deprecated-audit.tsv"
  prep_tmp_dir="$prep_out_dir/upstream-tmp"
  mkdir "$prep_tmp_dir"
  ZMIN_UPSTREAM_GIT_CACHE="$prep_manifest_cache" \
    ZMIN_UPSTREAM_GIT_TAG="$upstream_tag" \
    ZMIN_TMPDIR="$prep_tmp_dir" \
    bash "$prep_tools_root/tools/git-upstream-compat-manifest.sh" all-nondeprecated >"$prep_manifest"
  ZMIN_UPSTREAM_GIT_CACHE="$prep_manifest_cache" \
    ZMIN_UPSTREAM_GIT_TAG="$upstream_tag" \
    bash "$prep_tools_root/tools/git-upstream-deprecated-audit.sh" audit >"$prep_audit"
  prep_all_names="$prep_out_dir/all-names.txt"
  prep_selected_names="$prep_out_dir/selected-names.txt"
  prep_excluded_names="$prep_out_dir/excluded-names.txt"
  find "$prep_source/t" -maxdepth 1 -type f -name 't[0-9][0-9][0-9][0-9]-*.sh' -print |
    sed 's#^.*/##' | LC_ALL=C sort >"$prep_all_names"
  awk -F '\t' 'NR > 1 { print $2 }' "$prep_manifest" | LC_ALL=C sort >"$prep_selected_names"
  comm -23 "$prep_all_names" "$prep_selected_names" >"$prep_excluded_names"
  test "$(wc -l <"$prep_all_names" | tr -d ' ')" = 1046
  test "$(wc -l <"$prep_selected_names" | tr -d ' ')" = 1045
  test "$(cat "$prep_excluded_names")" = t5323-pack-redundant.sh
  prep_deprecated_excluded="$(awk -F '\t' 'NR > 1 && $1 ~ /^fully-excluded/ { print $2 }' "$prep_audit")"
  test "$prep_deprecated_excluded" = t5323-pack-redundant.sh
  prep_mixed_deprecated_count="$(awk -F '\t' 'NR > 1 && $1 == "mixed-in-scope" { count += 1 } END { print count + 0 }' "$prep_audit")"
  test "$prep_mixed_deprecated_count" -gt 0
  {
    printf 'result\tpass\n'
    printf 'archive_sha256\t%s\n' "$upstream_archive_sha256"
    printf 'marker\t%s\n' "$prep_source/.zmin-pristine-source.sha256"
    printf 'top_level_count\t1046\n'
    printf 'selected_count\t1045\n'
    printf 'sole_exclusion\tt5323-pack-redundant.sh\n'
    printf 'deprecated_fully_excluded\t%s\n' "$prep_deprecated_excluded"
    printf 'deprecated_mixed_in_scope\t%s\n' "$prep_mixed_deprecated_count"
  } >"$prep_out_dir/selftest-summary.tsv"
  write_parse_checksums "$prep_out_dir"
  exit 0
fi

if [[ "${1:-}" == --selftest-parse-lane ]]; then
  [[ "$#" == 6 ]] || die 'usage: --selftest-parse-lane <lane> <manifest.tsv> <logs-dir> <out-dir> <policy.tsv>'
  cli_lane="$2"
  cli_manifest="$3"
  cli_logs_dir="$4"
  cli_out_dir="$5"
  cli_policy_file="$6"
  [[ "$cli_lane" == control || "$cli_lane" == zmin ]] || die 'self-test lane must be control or zmin'
  [[ -f "$cli_manifest" && -d "$cli_logs_dir" ]] || die 'self-test manifest/log directory is missing'
  [[ -f "$cli_policy_file" ]] || die 'self-test policy file is missing'
  [[ "$cli_logs_dir" != "$cli_out_dir" ]] || die 'self-test output must be separate from logs'
  if [[ -e "$cli_out_dir" ]] && find "$cli_out_dir" -mindepth 1 -print -quit 2>/dev/null | grep -q .; then
    die 'self-test output directory is not empty'
  fi
  mkdir -p "$cli_out_dir"
  cli_optional_skips="$cli_out_dir/optional-skips.tsv"
  cli_assertion_skips="$cli_out_dir/assertion-skips.tsv"
  printf 'lane\ttest\tclassification\trequired_run_profile\treason\tlog_sha256\n' >"$cli_optional_skips"
  printf 'lane\ttest\tassertion_skip_count\tlog_sha256\n' >"$cli_assertion_skips"
  cli_parser_rc=0
  parse_lane_logs "$cli_lane" "$cli_manifest" "$cli_logs_dir" \
    "$cli_optional_skips" "$cli_assertion_skips" "$cli_policy_file" || cli_parser_rc=$?
  cli_outcome_rc=0
  cli_outcome_reason=pass
  if [[ "$cli_parser_rc" != 0 ]]; then
    cli_outcome_rc=2
    cli_outcome_reason=invalid-parser-input
  elif (( parse_missing_log_count > 0 )); then
    cli_outcome_rc=1
    cli_outcome_reason=invalid-missing-logs
  elif (( parse_top_level_skip_count > 0 )); then
    cli_outcome_rc=1
    cli_outcome_reason=incomplete-optional-skips
  fi
  {
    printf 'result\t%s\n' "$([[ "$cli_outcome_rc" == 0 ]] && printf pass || printf fail)"
    printf 'exit_code\t%s\n' "$cli_outcome_rc"
    printf 'reason\t%s\n' "$cli_outcome_reason"
    printf 'lane\t%s\n' "$cli_lane"
    printf 'missing_log_count\t%s\n' "${parse_missing_log_count:-0}"
    printf 'top_level_skip_count\t%s\n' "${parse_top_level_skip_count:-0}"
  } >"$cli_out_dir/outcome.tsv"
  write_parse_checksums "$cli_out_dir"
  exit "$cli_outcome_rc"
fi

[[ -n "$artifact_root" ]] || die "artifact root is required"
artifact_root="$(cd "$(dirname "$artifact_root")" && pwd)/$(basename "$artifact_root")"
run_root="$(dirname "$artifact_root")"
work_root="$run_root/work"
mkdir -p "$artifact_root" "$work_root"
repo_tmp="$repo_root/.tmp"
workspace_tmp_owned=0

cleanup() {
  local rc=$?
  set +e
  mkdir -p "$artifact_root"
  if [[ ! -f "$artifact_root/outcome.tsv" ]]; then
    {
      printf 'result\tfail\n'
      printf 'exit_code\t%s\n' "$rc"
      printf 'reason\tunexpected replay termination before outcome was written\n'
    } >"$artifact_root/outcome.tsv"
  fi
  if [[ ! -f "$artifact_root/metadata.tsv" ]]; then
    {
      printf 'workflow_commit\t%s\n' "${GITHUB_SHA:-unknown}"
      printf 'scope\tfull1045\n'
      printf 'classification\tdiagnostic\n'
      printf 'reason\tmetadata was incomplete at termination\n'
    } >"$artifact_root/metadata.tsv"
  fi
  if declare -F write_checksums >/dev/null 2>&1; then
    write_checksums
  else
    (
      cd "$artifact_root"
      while IFS= read -r -d '' file; do
        sha256sum "${file#./}"
      done < <(find . -type f ! -name checksums.sha256 -print0 | LC_ALL=C sort -z)
    ) >"$artifact_root/checksums.sha256"
  fi
  chmod -R u+w "$work_root" 2>/dev/null || true
  rm -rf -- "$work_root" 2>/dev/null || true
  if [[ "$workspace_tmp_owned" == 1 ]]; then
    rm -rf -- "$repo_tmp" 2>/dev/null || true
  fi
  trap - EXIT
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 143' INT TERM HUP

if find "$artifact_root" -mindepth 1 -print -quit 2>/dev/null | grep -q .; then
  die "artifact root is not empty; refusing to merge another run"
fi
if [[ -f "$run_root/dependency-preflight.tsv" ]]; then
  cp "$run_root/dependency-preflight.tsv" "$artifact_root/dependency-preflight.tsv"
fi
dependency_preflight_failure() {
  local message="$1"
  printf '%s\n' "$message" >"$artifact_root/failure.txt"
  {
    printf 'result\tfail\n'
    printf 'exit_code\t2\n'
    printf 'reason\t%s\n' "$message"
  } >"$artifact_root/outcome.tsv"
  exit 2
}
[[ -f "$run_root/dependency-preflight.tsv" ]] ||
  dependency_preflight_failure 'dependency preflight is missing; refusing to run without provisioned tools'
grep -Fqx $'dependency_preflight_complete\ttrue' "$run_root/dependency-preflight.tsv" ||
  dependency_preflight_failure 'dependency preflight is incomplete; refusing to run without provisioned tools'
if [[ -e "$repo_tmp" ]]; then
  die "checkout repository .tmp must be absent before replay"
fi
mkdir "$repo_tmp"
workspace_tmp_owned=1

[[ "$jobs" =~ ^[1-9][0-9]*$ ]] || die "jobs must be a positive decimal integer"
[[ "$test_timeout" =~ ^(0|[1-9][0-9]*)$ ]] || die "per-test timeout must be 0 or positive"
jobs_n=$((10#$jobs))
test_timeout_n=$((10#$test_timeout))
(( jobs_n <= 64 )) || die "jobs must not exceed 64"
(( test_timeout_n <= 86400 )) || die "per-test timeout must not exceed 86400 seconds"

classification=diagnostic
if [[ "$test_timeout_n" == 0 ]]; then
  classification=authoritative
fi
expected_tests=$expected_full_tests
suite_mode=all-nondeprecated

mkdir -p "$artifact_root/scope" "$artifact_root/control" "$artifact_root/zmin"
commit_sha="$(git -C "$repo_root" rev-parse HEAD)"
test "$commit_sha" = "${GITHUB_SHA:-$commit_sha}" || die "checkout is not triggering commit"

{
  printf 'workflow_commit\t%s\n' "$commit_sha"
  printf 'scope\tfull1045\n'
  printf 'profile\t%s\n' "$replay_profile"
  printf 'cross_platform_status\tpending\n'
  printf 'expected_tests\t%s\n' "$expected_tests"
  printf 'jobs\t%s\n' "$jobs_n"
  printf 'per_test_timeout_seconds\t%s\n' "$test_timeout_n"
  printf 'classification\t%s\n' "$classification"
  printf 'authority_rule\tfull1045 with per_test_timeout=0 only\n'
  printf 'upstream_tag\t%s\n' "$upstream_tag"
  printf 'upstream_commit\t%s\n' "$upstream_commit"
  printf 'upstream_tag_object\t%s\n' "$upstream_tag_object"
  printf 'upstream_archive_sha256\t%s\n' "$upstream_archive_sha256"
  printf 'no_retries_or_rerolls\ttrue\n'
  printf 'repository_writes\tnone; contents:read only\n'
  printf 'secrets\tnone\n'
  printf 'runner\tubuntu-24.04 x86_64\n'
} >"$artifact_root/metadata.tsv"

{
  printf 'field\tvalue\n'
  printf 'uname_a\t%s\n' "$(uname -a)"
  printf 'kernel\t%s\n' "$(uname -r)"
  printf 'arch\t%s\n' "$(uname -m)"
  printf 'bits\t%s\n' "$(getconf LONG_BIT)"
  printf 'runner_os\t%s\n' "${RUNNER_OS:-unknown}"
  printf 'runner_arch\t%s\n' "${RUNNER_ARCH:-unknown}"
  printf 'runner_image\t%s\n' "${ImageOS:-unknown}/${ImageVersion:-unknown}"
  printf 'df_root\t%s\n' "$(df -P / | tail -n 1)"
  printf 'os_release\t%s\n' "$(tr '\n' ';' </etc/os-release)"
} >"$artifact_root/runner.tsv"

write_checksums() {
  local checksum_file="$artifact_root/checksums.sha256"
  (
    cd "$artifact_root"
    while IFS= read -r -d '' file; do
      sha256sum "${file#./}"
    done < <(find . -type f ! -name checksums.sha256 -print0 | LC_ALL=C sort -z)
  ) >"$checksum_file"
}

fail_with_artifact() {
  local message="$1"
  local rc="${2:-2}"
  printf '%s\n' "$message" >"$artifact_root/failure.txt"
  {
    printf 'result\tfail\n'
    printf 'exit_code\t%s\n' "$rc"
    printf 'reason\t%s\n' "$message"
  } >"$artifact_root/outcome.tsv"
  write_checksums
  exit "$rc"
}

[[ "$rust_toolchain" == "$required_rust_toolchain" ]] ||
  fail_with_artifact "REPLAY_RUST_TOOLCHAIN must be exactly $required_rust_toolchain"

for command in awk cat comm curl cvs cvsps df diff du envsubst find gcc git grep gpg highlight iconv java jgit make msgfmt openssl p4 p4d perl python3 realpath rg sed sha256sum shasum sh ssh sort svn svnadmin svnmucc svnserve tail tar timeout tr uniq uname wc cc ar ranlib apache2 apache2ctl locale sudo; do
  command -v "$command" >/dev/null || fail_with_artifact "missing command: $command"
done

runtime_executables="$artifact_root/runtime-executables.tsv"
printf 'variable\tcommand\tpath\tcanonical\n' >"$runtime_executables"
require_canonical_executable() {
  local variable="$1"
  local command_name="$2"
  local path="$3"
  local canonical
  path_text_is_safe "$path" ||
    fail_with_artifact "$variable must be an existing absolute canonical executable"
  canonical="$(resolve_canonical_executable "$path" || true)"
  [[ "$canonical" == "$path" ]] ||
    fail_with_artifact "$variable must be an existing absolute non-symlink executable: $path"
  printf '%s\t%s\t%s\t%s\n' "$variable" "$command_name" "$path" "$canonical" \
    >>"$runtime_executables"
}

require_canonical_executable ZMIN_REPLAY_RUSTUP rustup "$rustup_bin"
zmin_test_perl="${ZMIN_TEST_PERL:-}"
contract_git_bin="${ZMIN_UPSTREAM_CONTRACT_GIT:-}"
contract_python_bin="${ZMIN_UPSTREAM_CONTRACT_PYTHON:-}"
contract_make_bin="${ZMIN_UPSTREAM_CONTRACT_MAKE:-}"
require_canonical_executable ZMIN_TEST_PERL perl "$zmin_test_perl"
require_canonical_executable ZMIN_UPSTREAM_CONTRACT_GIT git "$contract_git_bin"
require_canonical_executable ZMIN_UPSTREAM_CONTRACT_PYTHON python3 "$contract_python_bin"
require_canonical_executable ZMIN_UPSTREAM_CONTRACT_MAKE make "$contract_make_bin"
contract_shell_bin="${ZMIN_UPSTREAM_CONTRACT_SHELL:-}"
contract_cc_bin="${ZMIN_UPSTREAM_CONTRACT_CC:-}"
contract_ar_bin="${ZMIN_UPSTREAM_CONTRACT_AR:-}"
contract_ranlib_bin="${ZMIN_UPSTREAM_CONTRACT_RANLIB:-}"
contract_p4_bin="${ZMIN_UPSTREAM_CONTRACT_P4:-}"
contract_p4d_bin="${ZMIN_UPSTREAM_CONTRACT_P4D:-}"
contract_jgit_bin="${ZMIN_UPSTREAM_CONTRACT_JGIT:-}"
require_canonical_executable ZMIN_UPSTREAM_CONTRACT_SHELL sh "$contract_shell_bin"
require_canonical_executable ZMIN_UPSTREAM_CONTRACT_CC cc "$contract_cc_bin"
require_canonical_executable ZMIN_UPSTREAM_CONTRACT_AR ar "$contract_ar_bin"
require_canonical_executable ZMIN_UPSTREAM_CONTRACT_RANLIB ranlib "$contract_ranlib_bin"
require_canonical_executable ZMIN_UPSTREAM_CONTRACT_P4 p4 "$contract_p4_bin"
require_canonical_executable ZMIN_UPSTREAM_CONTRACT_P4D p4d "$contract_p4d_bin"
require_canonical_executable ZMIN_UPSTREAM_CONTRACT_JGIT jgit "$contract_jgit_bin"

contract_path="$repo_root/tools/git-current-compat-contract.json"
test -f "$contract_path" || fail_with_artifact "missing compatibility contract: $contract_path"
python3 - "$contract_path" >"$artifact_root/contract-validation.tsv" <<'PY'
import json
import sys


def unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def exact_keys(value, expected, path):
    if not isinstance(value, dict) or set(value) != set(expected):
        raise ValueError(f"unexpected keys at {path}: {sorted(value) if isinstance(value, dict) else type(value).__name__}")


def string(value, path):
    if not isinstance(value, str) or not value:
        raise ValueError(f"{path} must be a non-empty string")


with open(sys.argv[1], encoding="utf-8") as handle:
    contract = json.load(handle, object_pairs_hook=unique_pairs)

exact_keys(contract, {"contract_version", "upstream", "manifest", "skip_policy"}, "root")
if contract["contract_version"] != 2:
    raise ValueError("unsupported contract_version")
upstream = contract["upstream"]
manifest = contract["manifest"]
exact_keys(upstream, {"tag", "archive_url", "archive_sha256", "tag_object", "commit"}, "upstream")
exact_keys(manifest, {"mode", "top_level_count", "selected_count", "sole_exclusion", "top_level_names_sha256", "selected_names_sha256", "generated_tsv_sha256", "names_encoding"}, "manifest")
for key in ("tag", "archive_url", "archive_sha256", "tag_object", "commit"):
    string(upstream[key], f"upstream.{key}")
for key in ("mode", "sole_exclusion", "top_level_names_sha256", "selected_names_sha256", "generated_tsv_sha256", "names_encoding"):
    string(manifest[key], f"manifest.{key}")
if manifest["mode"] != "all-nondeprecated":
    raise ValueError("manifest mode must be all-nondeprecated")
if manifest["top_level_count"] != 1046 or manifest["selected_count"] != 1045:
    raise ValueError("contract denominator is not 1046/1045")
if manifest["sole_exclusion"] != "t5323-pack-redundant.sh":
    raise ValueError("contract sole exclusion is not t5323-pack-redundant.sh")
policy = contract["skip_policy"]
exact_keys(policy, {"version", "base_profile", "entries"}, "skip_policy")
if policy["version"] != 1 or policy["base_profile"] != "linux-ubuntu-24.04-x86_64":
    raise ValueError("unsupported skip policy binding")
expected_policy = [
    {"test": "t0029-core-unsetenvvars.sh", "classification": "platform", "required_run_profile": "windows-x86_64", "reason": "skipping Windows-specific tests"},
    {"test": "t0051-windows-named-pipe.sh", "classification": "platform", "required_run_profile": "windows-x86_64", "reason": "skipping Windows-specific tests"},
    {"test": "t1509-root-work-tree.sh", "classification": "dedicated-host", "required_run_profile": "linux-privileged-root", "reason": "Test requiring writable / skipped. Read this test if you want to run it"},
    {"test": "t3910-mac-os-precompose.sh", "classification": "platform", "required_run_profile": "macos-native", "reason": "filesystem does not corrupt utf-8"},
    {"test": "t5580-unc-paths.sh", "classification": "platform", "required_run_profile": "windows-x86_64", "reason": "skipping Windows-only path tests"},
    {"test": "t5608-clone-2gb.sh", "classification": "dedicated-host", "required_run_profile": "linux-high-disk", "reason": "expensive 2GB clone test; enable with GIT_TEST_CLONE_2GB=true"},
    {"test": "t6419-merge-ignorecase.sh", "classification": "platform", "required_run_profile": "linux-case-insensitive-fs", "reason": "skipping case insensitive tests - case sensitive file system"},
]
if policy["entries"] != expected_policy:
    raise ValueError("skip policy entries are not the audited exact sorted policy")
print(f"contract_version\t{contract['contract_version']}")
print(f"skip_policy_version\t{policy['version']}")
print(f"skip_policy_base_profile\t{policy['base_profile']}")
for entry in policy["entries"]:
    print("skip_policy_entry\t{test}\t{classification}\t{required_run_profile}\t{reason}".format(**entry))
for key in ("tag", "archive_url", "archive_sha256", "tag_object", "commit"):
    print(f"upstream_{key}\t{upstream[key]}")
for key in ("mode", "top_level_count", "selected_count", "sole_exclusion", "top_level_names_sha256", "selected_names_sha256", "generated_tsv_sha256"):
    print(f"manifest_{key}\t{manifest[key]}")
PY

contract_tag="$(awk -F '\t' '$1 == "upstream_tag" { print $2 }' "$artifact_root/contract-validation.tsv")"
contract_archive_url="$(awk -F '\t' '$1 == "upstream_archive_url" { print $2 }' "$artifact_root/contract-validation.tsv")"
contract_archive_sha256="$(awk -F '\t' '$1 == "upstream_archive_sha256" { print $2 }' "$artifact_root/contract-validation.tsv")"
contract_tag_object="$(awk -F '\t' '$1 == "upstream_tag_object" { print $2 }' "$artifact_root/contract-validation.tsv")"
contract_commit="$(awk -F '\t' '$1 == "upstream_commit" { print $2 }' "$artifact_root/contract-validation.tsv")"
test "$contract_tag" = "$upstream_tag" || fail_with_artifact "contract tag binding mismatch"
test "$contract_archive_url" = "$upstream_archive_url" || fail_with_artifact "contract archive URL binding mismatch"
test "$contract_archive_sha256" = "$upstream_archive_sha256" || fail_with_artifact "contract archive hash binding mismatch"
test "$contract_tag_object" = "$upstream_tag_object" || fail_with_artifact "contract tag object binding mismatch"
test "$contract_commit" = "$upstream_commit" || fail_with_artifact "contract commit binding mismatch"
test "$replay_profile" = "linux-ubuntu-24.04-x86_64" || fail_with_artifact "REPLAY_PROFILE is not the exact authoritative Linux profile"
skip_policy_file="$artifact_root/scope/skip-policy.tsv"
printf 'test\tclassification\trequired_run_profile\treason\n' >"$skip_policy_file"
awk -F '\t' '$1 == "skip_policy_entry" { print $2 "\t" $3 "\t" $4 "\t" $5 }' \
  "$artifact_root/contract-validation.tsv" >>"$skip_policy_file"
test "$(awk -F '\t' 'NR > 1 { count += 1 } END { print count + 0 }' "$skip_policy_file")" = 7 ||
  fail_with_artifact "skip policy does not contain exactly seven entries"
printf 'contract_sha256\t%s\n' "$(sha256sum "$contract_path" | awk '{ print $1 }')" >>"$artifact_root/contract-validation.tsv"
printf 'contract_sha256\t%s\n' "$(sha256sum "$contract_path" | awk '{ print $1 }')" >>"$artifact_root/metadata.tsv"

export CARGO_HOME="$work_root/cargo-home"
export CARGO_TERM_COLOR=never
export CARGO_HTTP_TIMEOUT="${CARGO_HTTP_TIMEOUT:-30}"
export RUSTUP_MAX_RETRIES="${RUSTUP_MAX_RETRIES:-0}"
export CARGO_NET_RETRY="${CARGO_NET_RETRY:-0}"
rust_toolchain_entry="$("$rustup_bin" toolchain list | awk -v toolchain="$rust_toolchain" '$1 == toolchain { print $1; exit }')"
test "$rust_toolchain_entry" = "$rust_toolchain" ||
  fail_with_artifact "required Rust toolchain is not installed: $rust_toolchain"
if ! rustc_verbose="$("$rustup_bin" run "$rust_toolchain" rustc --version --verbose 2>&1)"; then
  fail_with_artifact "could not execute rustc from required Rust toolchain: $rust_toolchain"
fi
rustc_release="$(printf '%s\n' "$rustc_verbose" | awk 'NR == 1 { print; exit }')"
test "$rustc_release" = 'rustc 1.98.0 (88d9e12ae 2026-08-18)' ||
  fail_with_artifact "unexpected rustc release for $rust_toolchain: $rustc_release"
rustc_host="$(printf '%s\n' "$rustc_verbose" | awk '$1 == "host:" { print $2; exit }')"
test "$rustc_host" = x86_64-unknown-linux-gnu ||
  fail_with_artifact "unexpected rustc host for $rust_toolchain: $rustc_host"
if ! cargo_version="$("$rustup_bin" run "$rust_toolchain" cargo --version 2>&1)"; then
  fail_with_artifact "could not execute cargo from required Rust toolchain: $rust_toolchain"
fi
case "$cargo_version" in
  'cargo 1.98.0 ('*) ;;
  *) fail_with_artifact "unexpected cargo release for $rust_toolchain: $cargo_version" ;;
esac
build_target="$work_root/cargo-target"
mkdir -p "$build_target"
build_log="$artifact_root/build.log"
set +e
CARGO_TARGET_DIR="$build_target" "$rustup_bin" run "$rust_toolchain" cargo build \
  --locked --release --manifest-path "$repo_root/Cargo.toml" \
  -p zmin-cli -p zmin-git-remote-http >"$build_log" 2>&1
build_rc=$?
set -e
if [[ "$build_rc" != 0 ]]; then
  fail_with_artifact "release-locked zmin build failed (exit $build_rc)" "$build_rc"
fi

zmin_bin="$build_target/release/zmin"
zmin_remote_http="$build_target/release/zmin-git-remote-http"
test -x "$zmin_bin" || fail_with_artifact "missing release zmin binary"
test -x "$zmin_remote_http" || fail_with_artifact "missing release zmin-git-remote-http binary"
{
  printf 'rust_toolchain\t%s\n' "$rust_toolchain"
  printf 'rustc_host\t%s\n' "$rustc_host"
  printf '%s\n' "$rustc_verbose" | sed 's/^/rustc\t/'
  printf 'cargo\t%s\n' "$cargo_version"
  sha256sum "$repo_root/Cargo.lock" | awk '{ print "Cargo.lock_sha256\t" $1 }'
  sha256sum "$zmin_bin" | awk '{ print "zmin_sha256\t" $1 }'
  sha256sum "$zmin_remote_http" | awk '{ print "zmin_git_remote_http_sha256\t" $1 }'
} >"$artifact_root/build-metadata.tsv"

refs_file="$artifact_root/upstream-tag-refs.txt"
refs_error="$artifact_root/upstream-tag-refs.stderr"
set +e
GIT_TERMINAL_PROMPT=0 timeout 120 git ls-remote "$upstream_repo_url" \
  "refs/tags/$upstream_tag" "refs/tags/$upstream_tag^{}" >"$refs_file" 2>"$refs_error"
refs_rc=$?
set -e
[[ "$refs_rc" == 0 ]] || fail_with_artifact "could not validate upstream tag refs (exit $refs_rc)" "$refs_rc"
remote_tag_object="$(awk -v ref="refs/tags/$upstream_tag" '$2 == ref { print $1; exit }' "$refs_file")"
remote_commit="$(awk -v ref="refs/tags/$upstream_tag^{}" '$2 == ref { print $1; exit }' "$refs_file")"
test "$remote_tag_object" = "$upstream_tag_object" || fail_with_artifact "upstream annotated tag object mismatch"
test "$remote_commit" = "$upstream_commit" || fail_with_artifact "upstream peeled tag commit mismatch"

archive="$work_root/$upstream_tag.tar.gz"
archive_tmp="$archive.tmp"
set +e
curl -fsSL --connect-timeout 30 --max-time 600 "$upstream_archive_url" -o "$archive_tmp"
archive_rc=$?
set -e
[[ "$archive_rc" == 0 ]] || fail_with_artifact "could not download upstream archive (exit $archive_rc)" "$archive_rc"
printf '%s  %s\n' "$upstream_archive_sha256" "$archive_tmp" | sha256sum -c - || fail_with_artifact "upstream archive SHA-256 mismatch"
tar -tzf "$archive_tmp" >/dev/null || fail_with_artifact "upstream archive is not a valid gzip tar"
mv -- "$archive_tmp" "$archive"

manifest_cache="$work_root/manifest-cache"
prepare_manifest_cache "$archive" "$manifest_cache" || fail_with_artifact "validated upstream archive/cache preparation failed"
archive="$manifest_cache/$upstream_tag.tar.gz"

{
  printf 'upstream_tag\t%s\n' "$upstream_tag"
  printf 'upstream_tag_object_expected\t%s\n' "$upstream_tag_object"
  printf 'upstream_tag_object_observed\t%s\n' "$remote_tag_object"
  printf 'upstream_commit_expected\t%s\n' "$upstream_commit"
  printf 'upstream_commit_observed\t%s\n' "$remote_commit"
  printf 'archive_url\t%s\n' "$upstream_archive_url"
  printf 'archive_sha256_expected\t%s\n' "$upstream_archive_sha256"
  printf 'archive_sha256_observed\t%s\n' "$(sha256sum "$archive" | awk '{ print $1 }')"
  printf 'pristine_marker\t%s\n' "$manifest_cache/git-$upstream_tag/.zmin-pristine-source.sha256"
  printf 'pristine_marker_sha256\t%s\n' "$(sha256sum "$manifest_cache/git-$upstream_tag/.zmin-pristine-source.sha256" | awk '{ print $1 }')"
} >"$artifact_root/upstream-materialization.tsv"

full_manifest="$artifact_root/scope/full1045.tsv"
ZMIN_UPSTREAM_GIT_CACHE="$manifest_cache" \
  ZMIN_UPSTREAM_GIT_TAG="$upstream_tag" \
  bash "$repo_root/tools/git-upstream-compat-manifest.sh" all-nondeprecated >"$full_manifest"

all_names="$work_root/all-names.txt"
selected_names="$work_root/selected-names.txt"
excluded_names="$artifact_root/scope/excluded-names.txt"
find "$manifest_cache/git-$upstream_tag/t" -maxdepth 1 -type f -name 't[0-9][0-9][0-9][0-9]-*.sh' -print |
  sed 's#^.*/##' | LC_ALL=C sort >"$all_names"
awk -F '\t' 'NR > 1 { print $2 }' "$full_manifest" | LC_ALL=C sort >"$selected_names"
comm -23 "$all_names" "$selected_names" >"$excluded_names"
comm -13 "$all_names" "$selected_names" >"$work_root/unexpected-selected.txt"
full_count="$(wc -l <"$selected_names" | tr -d ' ')"
all_count="$(wc -l <"$all_names" | tr -d ' ')"
excluded_count="$(wc -l <"$excluded_names" | tr -d ' ')"
contract_top_names_sha256="$(awk -F '\t' '$1 == "manifest_top_level_names_sha256" { print $2 }' "$artifact_root/contract-validation.tsv")"
contract_selected_names_sha256="$(awk -F '\t' '$1 == "manifest_selected_names_sha256" { print $2 }' "$artifact_root/contract-validation.tsv")"
contract_generated_tsv_sha256="$(awk -F '\t' '$1 == "manifest_generated_tsv_sha256" { print $2 }' "$artifact_root/contract-validation.tsv")"
test "$(sha256sum "$all_names" | awk '{ print $1 }')" = "$contract_top_names_sha256" || fail_with_artifact "top-level name-list digest does not match contract"
test "$(sha256sum "$selected_names" | awk '{ print $1 }')" = "$contract_selected_names_sha256" || fail_with_artifact "selected name-list digest does not match contract"
test "$(sha256sum "$full_manifest" | awk '{ print $1 }')" = "$contract_generated_tsv_sha256" || fail_with_artifact "generated manifest digest does not match contract"
printf 'manifest_sha256\t%s\n' "$(sha256sum "$full_manifest" | awk '{ print $1 }')" >>"$artifact_root/metadata.tsv"
test "$full_count" = "$expected_full_tests" || fail_with_artifact "full denominator is $full_count, expected 1045"
test "$all_count" = "$((expected_full_tests + 1))" || fail_with_artifact "source count is $all_count, expected 1046"
test "$excluded_count" = 1 || fail_with_artifact "manifest excludes $excluded_count files, expected only t5323"
test "$(cat "$excluded_names")" = t5323-pack-redundant.sh || fail_with_artifact "manifest excluded a file other than t5323"
test ! -s "$work_root/unexpected-selected.txt" || fail_with_artifact "manifest selected a file absent from source"

retained_families="$artifact_root/scope/retained-families.tsv"
printf 'family\tname\tsource_files\tselected_files\toptional_prerequisite\n' >"$retained_families"
declare -A retained_family_names=(
  [t91]=git-svn
  [t94]=git-cvsserver
  [t95]=gitweb
  [t96]=cvsimport
  [t98]=git-p4
)
for family in t91 t94 t95 t96 t98; do
  family_name="${retained_family_names[$family]}"
  source_family_count="$(awk -v prefix="$family" 'index($0, prefix) == 1 { count += 1 } END { print count + 0 }' "$all_names")"
  selected_family_count="$(awk -v prefix="$family" 'index($0, prefix) == 1 { count += 1 } END { print count + 0 }' "$selected_names")"
  test "$source_family_count" -gt 0 || fail_with_artifact "retained $family_name test files are missing"
  test "$selected_family_count" = "$source_family_count" || fail_with_artifact "retained $family_name test files were removed"
  prerequisite=none
  if [[ "$family" == t98 ]]; then
    prerequisite='external p4 client is not bundled; skipped assertions remain visible in raw logs'
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$family" "$family_name" "$source_family_count" "$selected_family_count" "$prerequisite" >>"$retained_families"
done

deprecated_audit="$artifact_root/scope/deprecated-audit.tsv"
ZMIN_UPSTREAM_GIT_CACHE="$manifest_cache" \
  ZMIN_UPSTREAM_GIT_TAG="$upstream_tag" \
  bash "$repo_root/tools/git-upstream-deprecated-audit.sh" audit >"$deprecated_audit"
deprecated_excluded="$(awk -F '\t' 'NR > 1 && $1 ~ /^fully-excluded/ { print $2 }' "$deprecated_audit")"
test "$deprecated_excluded" = t5323-pack-redundant.sh || fail_with_artifact "deprecated audit excluded a retained test"
mixed_deprecated_count="$(awk -F '\t' 'NR > 1 && $1 == "mixed-in-scope" { count += 1 } END { print count + 0 }' "$deprecated_audit")"
test "$mixed_deprecated_count" -gt 0 || fail_with_artifact "retained deprecated assertions were not observed"

cp "$full_manifest" "$artifact_root/scope/selected-manifest.tsv"
printf 'selected_tests\t%s\n' "$expected_tests" >"$artifact_root/scope/selected-count.tsv"

materialize_role_cache() {
  local role="$1"
  local role_cache="$work_root/$role/cache"
  test ! -e "$role_cache" || fail_with_artifact "lane cache is not fresh: $role_cache"
  mkdir -p "$role_cache"
  cp "$archive" "$role_cache/$upstream_tag.tar.gz"
}
materialize_role_cache control
materialize_role_cache zmin

optional_skips_file="$artifact_root/optional-skips.tsv"
assertion_skips_file="$artifact_root/assertion-skips.tsv"
printf 'lane\ttest\tclassification\trequired_run_profile\treason\tlog_sha256\n' >"$optional_skips_file"
printf 'lane\ttest\tassertion_skip_count\tlog_sha256\n' >"$assertion_skips_file"
top_level_skip_count=0
allowed_top_level_skip_count=0
unclassified_top_level_skip_count=0
reason_mismatch_count=0
profile_mismatch_count=0
missing_log_count=0
log_validation_rc=0

run_role() {
  local role="$1"
  local stock_control="$2"
  local role_cache="$work_root/$role/cache"
  local role_home="$work_root/$role/home"
  local role_tmp="$work_root/$role/tmp"
  local role_out="$artifact_root/$role"
  local role_log="$role_out/run.log"
  mkdir -p "$role_home" "$role_tmp" "$role_out"
  : >"$role_home/.gitconfig"

  local common_env=(
    "HOME=$role_home"
    "TMPDIR=$role_tmp"
    "GIT_CONFIG_GLOBAL=$role_home/.gitconfig"
    "GIT_CONFIG_SYSTEM=/dev/null"
    "GIT_CONFIG_NOSYSTEM=1"
    "GIT_TERMINAL_PROMPT=0"
    "ZMIN_TEST_PERL=$zmin_test_perl"
    "ZMIN_UPSTREAM_CONTRACT_GIT=$contract_git_bin"
    "ZMIN_UPSTREAM_CONTRACT_PYTHON=$contract_python_bin"
    "ZMIN_UPSTREAM_CONTRACT_MAKE=$contract_make_bin"
    "ZMIN_UPSTREAM_CONTRACT_SHELL=$contract_shell_bin"
    "ZMIN_UPSTREAM_CONTRACT_CC=$contract_cc_bin"
    "ZMIN_UPSTREAM_CONTRACT_AR=$contract_ar_bin"
    "ZMIN_UPSTREAM_CONTRACT_RANLIB=$contract_ranlib_bin"
    "ZMIN_UPSTREAM_CONTRACT_P4=$contract_p4_bin"
    "ZMIN_UPSTREAM_CONTRACT_P4D=$contract_p4d_bin"
    "ZMIN_UPSTREAM_CONTRACT_JGIT=$contract_jgit_bin"
    "REPLAY_PROFILE=$replay_profile"
    "GIT_TEST_HTTPD=true"
    "GIT_TEST_SVNSERVE=true"
    "GIT_TEST_SVN_HTTPD=true"
    "GIT_TEST_ALLOW_SUDO=YES"
    "GIT_TEST_CLONE_2GB=false"
    "IKNOWWHATIAMDOING=NO"
    "GIT_TEST_UTF8_LOCALE=${GIT_TEST_UTF8_LOCALE:-en_US.UTF-8}"
    "LANG=C"
    "LC_ALL=C"
    "TZ=UTC"
    "ZMIN_UPSTREAM_GIT_TAG=$upstream_tag"
    "ZMIN_UPSTREAM_GIT_CACHE=$role_cache"
    "ZMIN_UPSTREAM_OUT_DIR=$role_out"
    "ZMIN_UPSTREAM_TEST_LIST=$full_manifest"
    "ZMIN_UPSTREAM_JOBS=$jobs_n"
    "ZMIN_UPSTREAM_TEST_FLAGS=-q"
    "ZMIN_UPSTREAM_TEST_TIMEOUT=$test_timeout_n"
    "ZMIN_UPSTREAM_ALLOW_FAILURES=0"
    "ZMIN_UPSTREAM_BOUNDED_RUN=0"
  )
  if [[ "$stock_control" == 1 ]]; then
    common_env+=("ZMIN_UPSTREAM_STOCK_GIT_CONTROL=1")
  else
    common_env+=("ZMIN_UPSTREAM_STOCK_GIT_CONTROL=0" "ZMIN_BIN=$zmin_bin")
  fi

  set +e
  env "${common_env[@]}" bash "$repo_root/tools/git-upstream-compat-suite.sh" "$suite_mode" >"$role_log" 2>&1
  local role_rc=$?
  set -e

  local summary_rows=0
  local summary_passes=0
  local summary_failures=0
  if [[ -f "$role_out/summary.tsv" ]]; then
    summary_rows="$(awk -F '\t' 'NR > 1 && NF >= 3 { count += 1 } END { print count + 0 }' "$role_out/summary.tsv")"
    summary_passes="$(awk -F '\t' 'NR > 1 && $3 == "pass" { count += 1 } END { print count + 0 }' "$role_out/summary.tsv")"
    summary_failures="$(awk -F '\t' 'NR > 1 && $3 == "fail" { count += 1 } END { print count + 0 }' "$role_out/summary.tsv")"
  fi
  {
    printf 'role\t%s\n' "$role"
    printf 'stock_control\t%s\n' "$stock_control"
    printf 'command_exit\t%s\n' "$role_rc"
    if [[ -f "$role_out/summary.tsv" ]]; then
      printf 'summary_present\ttrue\n'
    else
      printf 'summary_present\tfalse\n'
    fi
    printf 'summary_rows\t%s\n' "$summary_rows"
    printf 'summary_passes\t%s\n' "$summary_passes"
    printf 'summary_failures\t%s\n' "$summary_failures"
    printf 'expected_rows\t%s\n' "$expected_tests"
  } >"$role_out/status.tsv"
  printf '%s\n' "$role_rc"
}

stock_rc="$(run_role control 1)"
if ! parse_lane_logs control "$full_manifest" "$artifact_root/control" \
  "$optional_skips_file" "$assertion_skips_file" "$skip_policy_file"; then
  log_validation_rc=1
fi
missing_log_count=$((missing_log_count + parse_missing_log_count))
top_level_skip_count=$((top_level_skip_count + parse_top_level_skip_count))
allowed_top_level_skip_count=$((allowed_top_level_skip_count + parse_allowed_top_level_skip_count))
unclassified_top_level_skip_count=$((unclassified_top_level_skip_count + parse_unclassified_top_level_skip_count))
reason_mismatch_count=$((reason_mismatch_count + parse_reason_mismatch_count))
profile_mismatch_count=$((profile_mismatch_count + parse_profile_mismatch_count))
(( parse_missing_log_count == 0 )) || log_validation_rc=1
zmin_rc="$(run_role zmin 0)"
if ! parse_lane_logs zmin "$full_manifest" "$artifact_root/zmin" \
  "$optional_skips_file" "$assertion_skips_file" "$skip_policy_file"; then
  log_validation_rc=1
fi
missing_log_count=$((missing_log_count + parse_missing_log_count))
top_level_skip_count=$((top_level_skip_count + parse_top_level_skip_count))
allowed_top_level_skip_count=$((allowed_top_level_skip_count + parse_allowed_top_level_skip_count))
unclassified_top_level_skip_count=$((unclassified_top_level_skip_count + parse_unclassified_top_level_skip_count))
reason_mismatch_count=$((reason_mismatch_count + parse_reason_mismatch_count))
profile_mismatch_count=$((profile_mismatch_count + parse_profile_mismatch_count))
(( parse_missing_log_count == 0 )) || log_validation_rc=1

retained_family_skips="$artifact_root/scope/retained-family-skip-audit.tsv"
printf 'family\tname\tlane\tfully_skipped_top_level_tests\n' >"$retained_family_skips"
retained_top_level_skip_count=0
for family in t91 t94 t95 t96 t98; do
  family_name="${retained_family_names[$family]}"
  for lane in control zmin; do
    family_skip_count="$(awk -F '\t' -v family="$family" -v lane="$lane" \
      '$1 == lane && index($2, family) == 1 && $3 != "missing-log" { count += 1 } END { print count + 0 }' \
      "$optional_skips_file")"
    printf '%s\t%s\t%s\t%s\n' "$family" "$family_name" "$lane" "$family_skip_count" >>"$retained_family_skips"
    retained_top_level_skip_count=$((retained_top_level_skip_count + family_skip_count))
  done
done

coverage_gap_count="$(awk -F '\t' 'NR > 1 && $3 ~ /^(platform|dedicated-host)$/ { print $2 }' \
  "$optional_skips_file" | LC_ALL=C sort -u | wc -l | tr -d ' ')"
closure_gap_count="$coverage_gap_count"

resolve_stock_binary() {
  local lane_cache="$1"
  local candidate
  local canonical
  local candidate_count=0
  local resolved=""
  while IFS= read -r -d '' candidate; do
    case "$candidate" in
      "$lane_cache"/harness-v2.55.0-*/git)
        candidate_count=$((candidate_count + 1))
        resolved="$candidate"
        ;;
    esac
  done < <(find "$lane_cache" -mindepth 2 -maxdepth 2 -type f -name git -perm -111 -print0)
  if [[ "$candidate_count" != 1 ]]; then
    printf 'expected exactly one executable harness-v2.55.0-*/git, found %s\n' "$candidate_count" \
      >"$artifact_root/control/stock-discovery-error.txt"
    return 1
  fi
  canonical="$(realpath -e "$resolved")" || return 1
  stock_binary="$canonical"
  stock_version="$("$stock_binary" --version 2>&1)"
  if [[ "$stock_version" != 'git version 2.55.0' ]]; then
    printf 'stock Git version mismatch: %s\n' "$stock_version" \
      >"$artifact_root/control/stock-discovery-error.txt"
    return 1
  fi
  {
    printf 'stock_git_path\t%s\n' "$stock_binary"
    printf 'stock_git_sha256\t%s\n' "$(sha256sum "$stock_binary" | awk '{ print $1 }')"
    printf 'stock_git_version\t%s\n' "$stock_version"
    printf 'stock_git_candidate_count\t%s\n' "$candidate_count"
  } >"$artifact_root/control/binary-metadata.tsv"
  return 0
}

stock_binary=missing
stock_version=missing
stock_discovery_rc=0
if ! resolve_stock_binary "$work_root/control/cache"; then
  stock_discovery_rc=1
  printf 'stock_git_path\tmissing\nstock_git_candidate_count\tinvalid\n' >"$artifact_root/control/binary-metadata.tsv"
fi
{
  printf 'zmin_sha256\t%s\n' "$(sha256sum "$zmin_bin" | awk '{ print $1 }')"
  printf 'zmin_git_remote_http_sha256\t%s\n' "$(sha256sum "$zmin_remote_http" | awk '{ print $1 }')"
  printf 'zmin_version\t%s\n' "$("$zmin_bin" --version 2>&1)"
} >"$artifact_root/zmin/binary-metadata.tsv"

outcome_rc=0
outcome_reason=pass
if [[ "$stock_rc" != 0 || "$zmin_rc" != 0 ]]; then
  outcome_rc=1
  outcome_reason=replay-failure
fi
if [[ "$stock_discovery_rc" != 0 || "$stock_version" != 'git version 2.55.0' ]]; then
  outcome_rc=1
  outcome_reason=stock-discovery-failure
fi
for role in control zmin; do
  role_rows="$(awk -F '\t' '$1 == "summary_rows" { print $2 }' "$artifact_root/$role/status.tsv")"
  if [[ "$role_rows" != "$expected_tests" ]]; then
    outcome_rc=1
    outcome_reason=invalid-summary
  fi
done
if [[ "$log_validation_rc" != 0 || "$missing_log_count" != 0 ]]; then
  outcome_rc=1
  outcome_reason=invalid-missing-logs
elif [[ "$retained_top_level_skip_count" != 0 || "$unclassified_top_level_skip_count" != 0 ||
  "$reason_mismatch_count" != 0 || "$closure_gap_count" != 7 ]]; then
  outcome_rc=1
  outcome_reason=incomplete-optional-skips
fi
if [[ "$top_level_skip_count" != "$allowed_top_level_skip_count" ]]; then
  outcome_rc=1
  outcome_reason=incomplete-optional-skips
fi

artifact_bytes="$(du -sb "$artifact_root" | awk '{ print $1 }')"
if (( artifact_bytes > artifact_budget_bytes )); then
  outcome_rc=1
  budget_result=exceeded
else
  budget_result=within
fi
{
  if [[ "$outcome_rc" == 0 ]]; then
    printf 'result\tpass\n'
  else
    printf 'result\tfail\n'
  fi
  printf 'exit_code\t%s\n' "$outcome_rc"
  printf 'classification\t%s\n' "$classification"
  printf 'reason\t%s\n' "$outcome_reason"
  printf 'stock_control_exit\t%s\n' "$stock_rc"
  printf 'stock_discovery_exit\t%s\n' "$stock_discovery_rc"
  printf 'stock_git_version\t%s\n' "$stock_version"
  printf 'zmin_exit\t%s\n' "$zmin_rc"
  printf 'missing_log_count\t%s\n' "$missing_log_count"
  printf 'top_level_skip_count\t%s\n' "$top_level_skip_count"
  printf 'allowed_top_level_skip_count\t%s\n' "$allowed_top_level_skip_count"
  printf 'unclassified_top_level_skip_count\t%s\n' "$unclassified_top_level_skip_count"
  printf 'reason_mismatch_count\t%s\n' "$reason_mismatch_count"
  printf 'profile_mismatch_count\t%s\n' "$profile_mismatch_count"
  printf 'closure_gap_count\t%s\n' "$closure_gap_count"
  printf 'cross_platform_status\tpending\n'
  printf 'retained_top_level_skip_count\t%s\n' "$retained_top_level_skip_count"
  printf 'artifact_bytes\t%s\n' "$artifact_bytes"
  printf 'artifact_budget_bytes\t%s\n' "$artifact_budget_bytes"
  printf 'artifact_budget\t%s\n' "$budget_result"
  printf 'retry_or_reroll\tnone\n'
} >"$artifact_root/outcome.tsv"

write_checksums
exit "$outcome_rc"
