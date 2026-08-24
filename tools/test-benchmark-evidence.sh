#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
source "$repo_root/tools/benchmark-environment.sh"
tmp_root="$(mktemp -d /tmp/zmin-benchmark-evidence-test.XXXXXX)"
trap 'rm -rf "$tmp_root"' EXIT

expect_failure() {
  local label="$1" expected="$2"
  shift 2
  local output status
  set +e
  output="$("$@" 2>&1)"
  status=$?
  set -e
  [[ "$status" -ne 0 ]] || {
    printf '%s unexpectedly succeeded\n' "$label" >&2
    return 1
  }
  [[ "$output" == *"$expected"* ]] || {
    printf '%s diagnostic mismatch: %s\n' "$label" "$output" >&2
    return 1
  }
}

bundle="$tmp_root/bundle"
mkdir -p "$bundle"
expect_failure missing-bundle 'explicit ZMIN_GIT_HTTP_BUNDLE' \
  benchmark_validate_authoritative_git_comparator "$repo_root" "$tmp_root/git"
expect_failure git-mismatch 'ZMIN_STOCK_GIT must equal' \
  env ZMIN_GIT_HTTP_BUNDLE="$bundle" ZMIN_STOCK_GIT="$tmp_root/other/git" \
  bash -c 'source "$1/tools/benchmark-environment.sh"; benchmark_validate_authoritative_git_comparator "$1" "$2"' \
  benchmark-evidence-test "$repo_root" "$bundle/git"
expect_failure helper-mismatch 'comparator helper is missing or invalid' \
  env ZMIN_GIT_HTTP_BUNDLE="$bundle" \
  bash -c 'source "$1/tools/benchmark-environment.sh"; benchmark_validate_authoritative_git_comparator "$1" "$2"' \
  benchmark-evidence-test "$repo_root" "$bundle/git"

expected_exploratory_git=""
if [[ -x /usr/bin/git ]]; then
  expected_exploratory_git=/usr/bin/git
else
  expected_exploratory_git="$(command -v git)"
fi
exploratory_git="$(env -u ZMIN_STOCK_GIT -u GIT_BIN bash -c \
  'source "$1/tools/benchmark-environment.sh"; benchmark_resolve_observed_git' \
  benchmark-evidence-test "$repo_root")"
[[ "$exploratory_git" == "$expected_exploratory_git" ]] || {
  printf 'exploratory stock Git selection mismatch: expected %s, got %s\n' \
    "$expected_exploratory_git" "$exploratory_git" >&2
  exit 1
}
explicit_git="$(env ZMIN_STOCK_GIT=/bin/sh GIT_BIN=/bin/false bash -c \
  'source "$1/tools/benchmark-environment.sh"; benchmark_resolve_observed_git' \
  benchmark-evidence-test "$repo_root")"
[[ "$explicit_git" == /bin/sh ]] || {
  printf 'explicit observed stock Git selection mismatch: %s\n' "$explicit_git" >&2
  exit 1
}

set +e
standard_output="$({
  env ZMIN_BENCH_EVIDENCE_MODE=authoritative GIT_BIN= \
    "$repo_root/tools/git-performance-bench.sh" 2>&1
} )"
standard_status=$?
observed_output="$({
  env ZMIN_OBSERVED_BENCH_EVIDENCE_MODE=authoritative ZMIN_BIN=/bin/true ZMIN_STOCK_GIT= \
    "$repo_root/tools/git-observed-client-bench.sh" "$repo_root" 2>&1
} )"
observed_status=$?
set -e
[[ "$standard_status" -ne 0 && "$standard_output" == *'explicit pinned GIT_BIN'* ]] || {
  printf 'standard authoritative implicit Git fallback was not rejected: %s\n' "$standard_output" >&2
  exit 1
}
[[ "$observed_status" -ne 0 && "$observed_output" == *'explicit pinned ZMIN_STOCK_GIT'* ]] || {
  printf 'observed authoritative implicit Git fallback was not rejected: %s\n' "$observed_output" >&2
  exit 1
}

printf 'benchmark evidence preflight tests passed\n'
