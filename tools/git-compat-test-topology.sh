#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-compat-test-topology.sh [summary|audit]

Prints the current compatibility-test topology so upstream Git shell tests stay
authoritative and local Rust compat suites stay intentionally scoped.

Modes:
  summary   print machine-readable counts by topology bucket
  audit     print the full local suite classification and upstream scope counts
EOF
}

mode="${1:-summary}"
case "$mode" in
  summary|audit) ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tests_dir="$repo_root/crates/zmin-cli/tests"
legacy_excludes="$repo_root/tools/git-upstream-compat-tests-legacy-excludes.tsv"
file_excludes="$repo_root/tools/git-upstream-compat-tests-file-excludes.tsv"
manifest_tool="$repo_root/tools/git-upstream-compat-manifest.sh"

classify_suite() {
  local suite="$1"
  case "$suite" in
    git_observed_client_compat.rs)
      printf '%s\n' "observed-client"
      ;;
    git_replacement_dogfood_compat.rs|git_rev_parse_dogfood_compat.rs)
      printf '%s\n' "replace-git-dogfood"
      ;;
    git_lfs_local_compat.rs)
      printf '%s\n' "local-lfs-workflow"
      ;;
    git_cms_porcelain_compat.rs)
      printf '%s\n' "zmin-only-surface"
      ;;
    *_invalid_compat.rs)
      printf '%s\n' "invalid-input-oracle"
      ;;
    *)
      printf '%s\n' "focused-stock-oracle"
      ;;
  esac
}

role_reason() {
  local role="$1"
  case "$role" in
    observed-client)
      printf '%s\n' "exact IDE/plugin/client command families observed outside upstream shell tests"
      ;;
    replace-git-dogfood)
      printf '%s\n' "wrapper and alias smoke for practical replace-git workflows"
      ;;
    local-lfs-workflow)
      printf '%s\n' "LFS filter-process workflow not represented by stock upstream full-core shell scope"
      ;;
    zmin-only-surface)
      printf '%s\n' "Zmin extension kept outside the Git compatibility denominator"
      ;;
    invalid-input-oracle)
      printf '%s\n' "exact parser and fatal stderr parity probes that stay cheaper than broad upstream replay"
      ;;
    focused-stock-oracle)
      printf '%s\n' "focused stock-Git oracle/regression coverage; candidate to shrink only when upstream evidence fully subsumes the lane"
      ;;
    *)
      printf '%s\n' "unclassified"
      ;;
  esac
}

local_suites_file="$(mktemp "${TMPDIR:-/tmp}/zmin-local-compat-suites.XXXXXX")"
observed_file="$(mktemp "${TMPDIR:-/tmp}/zmin-local-compat-observed.XXXXXX")"
dogfood_file="$(mktemp "${TMPDIR:-/tmp}/zmin-local-compat-dogfood.XXXXXX")"
lfs_file="$(mktemp "${TMPDIR:-/tmp}/zmin-local-compat-lfs.XXXXXX")"
zmin_only_file="$(mktemp "${TMPDIR:-/tmp}/zmin-local-compat-zmin-only.XXXXXX")"
invalid_file="$(mktemp "${TMPDIR:-/tmp}/zmin-local-compat-invalid.XXXXXX")"
oracle_file="$(mktemp "${TMPDIR:-/tmp}/zmin-local-compat-oracle.XXXXXX")"
trap 'rm -f "$local_suites_file" "$observed_file" "$dogfood_file" "$lfs_file" "$zmin_only_file" "$invalid_file" "$oracle_file"' EXIT

find "$tests_dir" -maxdepth 1 -type f -name 'git_*_compat.rs' -print |
  sed 's#^.*/##' |
  LC_ALL=C sort >"$local_suites_file"

local_suite_count="$(awk 'END { print NR + 0 }' "$local_suites_file")"

if [[ "$local_suite_count" -eq 0 ]]; then
  echo "no local compat suites found under $tests_dir" >&2
  exit 2
fi

observed_count=0
dogfood_count=0
lfs_count=0
zmin_only_count=0
invalid_count=0
oracle_count=0

while IFS= read -r suite; do
  role="$(classify_suite "$suite")"
  case "$role" in
    observed-client)
      observed_count=$(( observed_count + 1 ))
      printf '%s\n' "$suite" >>"$observed_file"
      ;;
    replace-git-dogfood)
      dogfood_count=$(( dogfood_count + 1 ))
      printf '%s\n' "$suite" >>"$dogfood_file"
      ;;
    local-lfs-workflow)
      lfs_count=$(( lfs_count + 1 ))
      printf '%s\n' "$suite" >>"$lfs_file"
      ;;
    zmin-only-surface)
      zmin_only_count=$(( zmin_only_count + 1 ))
      printf '%s\n' "$suite" >>"$zmin_only_file"
      ;;
    invalid-input-oracle)
      invalid_count=$(( invalid_count + 1 ))
      printf '%s\n' "$suite" >>"$invalid_file"
      ;;
    focused-stock-oracle)
      oracle_count=$(( oracle_count + 1 ))
      printf '%s\n' "$suite" >>"$oracle_file"
      ;;
    *)
      echo "unclassified suite role for $suite" >&2
      exit 2
      ;;
  esac
done <"$local_suites_file"

legacy_excluded_count="$(awk -F '\t' 'NR > 1 && NF >= 3 { sum += $3 } END { print sum + 0 }' "$legacy_excludes")"
file_excluded_count="$(awk -F '\t' 'NR > 1 && NF >= 1 && $1 != "" { sum += 1 } END { print sum + 0 }' "$file_excludes")"
full_core_count="$("$manifest_tool" full-core | awk -F '\t' 'NR > 1 { count += 1 } END { print count + 0 }')"
upstream_total_count=$(( full_core_count + legacy_excluded_count + file_excluded_count ))

if [[ "$mode" == "summary" ]]; then
  printf 'bucket\tcount\treason\n'
  printf 'upstream_total_shell_files\t%s\t%s\n' \
    "$upstream_total_count" \
    'top-level upstream tNNNN shell files from the pinned cache'
  printf 'upstream_full_core_shell_files\t%s\t%s\n' \
    "$full_core_count" \
    'generated full-core upstream manifest after explicit excludes'
  printf 'upstream_explicit_family_excludes\t%s\t%s\n' \
    "$legacy_excluded_count" \
    'legacy, deprecated or external upstream families kept out of full-core'
  if [[ "$file_excluded_count" -gt 0 ]]; then
    printf 'upstream_explicit_file_excludes\t%s\t%s\n' \
      "$file_excluded_count" \
      'optional per-file upstream exclusions'
  fi
  printf 'local_compat_suites\t%s\t%s\n' \
    "$local_suite_count" \
    'Rust compat suites intentionally kept alongside upstream shell coverage'
  printf 'local_observed-client\t%s\t%s\n' "$observed_count" "$(role_reason observed-client)"
  printf 'local_replace-git-dogfood\t%s\t%s\n' "$dogfood_count" "$(role_reason replace-git-dogfood)"
  printf 'local_local-lfs-workflow\t%s\t%s\n' "$lfs_count" "$(role_reason local-lfs-workflow)"
  printf 'local_zmin-only-surface\t%s\t%s\n' "$zmin_only_count" "$(role_reason zmin-only-surface)"
  printf 'local_invalid-input-oracle\t%s\t%s\n' "$invalid_count" "$(role_reason invalid-input-oracle)"
  printf 'local_focused-stock-oracle\t%s\t%s\n' "$oracle_count" "$(role_reason focused-stock-oracle)"
  exit 0
fi

printf 'upstream_bucket\tcount\treason\n'
printf 'total_shell_files\t%s\t%s\n' \
  "$upstream_total_count" \
  'top-level upstream tNNNN shell files in the pinned cache'
printf 'full_core_shell_files\t%s\t%s\n' \
  "$full_core_count" \
  'generated full-core manifest used for exhaustive upstream replay'
printf 'explicit_family_excludes\t%s\t%s\n' \
  "$legacy_excluded_count" \
  'legacy, deprecated or external upstream families excluded from full-core'
printf 'explicit_file_excludes\t%s\t%s\n' \
  "$file_excluded_count" \
  'optional per-file upstream exclusions'
printf '\n'
printf 'local_role\tcount\treason\n'
printf 'observed-client\t%s\t%s\n' "$observed_count" "$(role_reason observed-client)"
printf 'replace-git-dogfood\t%s\t%s\n' "$dogfood_count" "$(role_reason replace-git-dogfood)"
printf 'local-lfs-workflow\t%s\t%s\n' "$lfs_count" "$(role_reason local-lfs-workflow)"
printf 'zmin-only-surface\t%s\t%s\n' "$zmin_only_count" "$(role_reason zmin-only-surface)"
printf 'invalid-input-oracle\t%s\t%s\n' "$invalid_count" "$(role_reason invalid-input-oracle)"
printf 'focused-stock-oracle\t%s\t%s\n' "$oracle_count" "$(role_reason focused-stock-oracle)"
printf '\n'
printf 'local_role\tsuite\treason\n'
for role in observed-client replace-git-dogfood local-lfs-workflow zmin-only-surface invalid-input-oracle focused-stock-oracle; do
  case "$role" in
    observed-client) role_file="$observed_file" ;;
    replace-git-dogfood) role_file="$dogfood_file" ;;
    local-lfs-workflow) role_file="$lfs_file" ;;
    zmin-only-surface) role_file="$zmin_only_file" ;;
    invalid-input-oracle) role_file="$invalid_file" ;;
    focused-stock-oracle) role_file="$oracle_file" ;;
  esac
  while IFS= read -r suite; do
    [[ -z "$suite" ]] && continue
    printf '%s\t%s\t%s\n' "$role" "$suite" "$(role_reason "$role")"
  done <"$role_file"
done
