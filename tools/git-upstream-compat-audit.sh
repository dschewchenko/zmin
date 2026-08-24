#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-upstream-compat-audit.sh [family-summary|legacy-audit|contract-check]

Audits the pinned upstream Git shell-suite scope without vendoring the upstream
t/ tree into this repository.

Modes:
  family-summary  print per-family top-level test counts (tNNxx groups)
  legacy-audit    verify explicit legacy excludes against the upstream tree and
                  show nearby t9x families that remain in scope
  contract-check  verify the frozen current-Git contract, source archive,
                  classifications and generated all-nondeprecated denominator

Environment:
  ZMIN_UPSTREAM_GIT_TAG         Upstream Git tag. Default: v2.55.0.
  ZMIN_UPSTREAM_GIT_CACHE       Cache dir for upstream Git source/build.
  ZMIN_UPSTREAM_COMPAT_CONTRACT Contract TSV. Default:
                                tools/git-upstream-compat-contract.tsv
  ZMIN_UPSTREAM_LEGACY_EXCLUDES Exclude TSV. Default:
                                tools/git-upstream-compat-tests-legacy-excludes.tsv
EOF
}

mode="${1:-}"
case "$mode" in
  family-summary|legacy-audit|contract-check) ;;
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
tag="${ZMIN_UPSTREAM_GIT_TAG:-v2.55.0}"
cache_root="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
source_dir="$cache_root/git-$tag"
legacy_excludes="${ZMIN_UPSTREAM_LEGACY_EXCLUDES:-$repo_root/tools/git-upstream-compat-tests-legacy-excludes.tsv}"
contract_file="${ZMIN_UPSTREAM_COMPAT_CONTRACT:-$repo_root/tools/git-upstream-compat-contract.tsv}"

contract_value() {
  local key="$1"
  awk -F '\t' -v key="$key" '$1 == key { print $2; exit }' "$contract_file"
}

if [[ ! -d "$source_dir/t" ]]; then
  echo "missing upstream Git source tree: $source_dir" >&2
  echo "run tools/git-upstream-compat-suite.sh once or populate ZMIN_UPSTREAM_GIT_CACHE first" >&2
  exit 2
fi

if [[ ! -f "$legacy_excludes" ]]; then
  echo "missing legacy exclude manifest: $legacy_excludes" >&2
  exit 2
fi

family_summary() {
  printf 'family\tcount\n'
  find "$source_dir/t" -maxdepth 1 -type f -name 't[0-9][0-9][0-9][0-9]-*.sh' -print |
    sed 's#^.*/##' |
    LC_ALL=C sort |
    cut -c1-3 |
    uniq -c |
    awk '{printf "%sxx\t%s\n", $2, $1}'
}

legacy_audit() {
  printf 'family\tpattern\texpected_count\tactual_count\tstatus\tclassification\tscope_decision\treason\tevidence\n'
  local audit_status=0
  while IFS=$'\t' read -r family pattern expected_count reason classification scope_decision evidence; do
      actual_count="$(
        find "$source_dir/t" -maxdepth 1 -type f -name "${pattern}*.sh" -print |
          sed '/^$/d' |
          wc -l |
          tr -d ' '
      )"
      status="match"
      if [[ "$actual_count" != "$expected_count" ]]; then
        status="count-mismatch"
        audit_status=1
      fi
      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$family" \
        "$pattern" \
        "$expected_count" \
        "$actual_count" \
        "$status" \
        "$classification" \
        "$scope_decision" \
        "$reason" \
        "$evidence"
  done < <(
    awk -F '\t' '
      NR == 1 || /^#/ || NF < 7 { next }
      { print $1 "\t" $2 "\t" $3 "\t" $4 "\t" $5 "\t" $6 "\t" $7 }
    ' "$legacy_excludes"
  )

  if [[ "$audit_status" != "0" ]]; then
    return 1
  fi

  printf '\n'
  printf 'in_scope_family\tcount\texample\tdescription\n'
  find "$source_dir/t" -maxdepth 1 -type f \( \
      -name 't90[0-9][0-9]-*.sh' -o \
      -name 't92[0-9][0-9]-*.sh' -o \
      -name 't93[0-9][0-9]-*.sh' -o \
      -name 't97[0-9][0-9]-*.sh' -o \
      -name 't99[0-9][0-9]-*.sh' \
    \) -print |
    sed 's#^.*/##' |
    LC_ALL=C sort |
    while IFS= read -r test_name; do
      family="${test_name:0:3}xx"
      description="$(
        sed -n '1,14p' "$source_dir/t/$test_name" |
          awk -F"'" '/^test_description=/{print $2; exit}'
      )"
      printf '%s\t%s\t%s\t%s\n' "$family" "1" "$test_name" "$description"
    done |
    awk -F '\t' '
      {
        counts[$1]++
        if (!($1 in example)) {
          example[$1] = $3
          description[$1] = $4
        }
      }
      END {
        for (family in counts) {
          printf "%s\t%s\t%s\t%s\n", family, counts[family], example[family], description[family]
        }
      }
    ' |
    LC_ALL=C sort
}

contract_check() {
  if [[ ! -f "$contract_file" ]]; then
    echo "missing compatibility contract: $contract_file" >&2
    return 2
  fi

  local contract_tag contract_repo contract_commit contract_sha contract_policy contract_mode contract_archive_url
  local expected_deprecated_groups expected_external_groups
  local expected_total expected_deprecated expected_denominator
  local expected_core expected_external source_dir_for_contract archive actual_sha
  contract_tag="$(contract_value upstream_git_tag)"
  contract_repo="$(contract_value upstream_git_repo)"
  contract_commit="$(contract_value upstream_git_commit)"
  contract_policy="$(contract_value source_identity_policy)"
  contract_archive_url="$(contract_value upstream_archive_url)"
  contract_sha="$(contract_value upstream_archive_sha256)"
  contract_mode="$(contract_value authoritative_manifest)"
  expected_deprecated_groups="$(contract_value deprecated_removed_groups)"
  expected_external_groups="$(contract_value external_current_groups)"
  expected_total="$(contract_value upstream_top_level_shell_tests)"
  expected_deprecated="$(contract_value excluded_deprecated_removed_shell_tests)"
  expected_denominator="$(contract_value authoritative_upstream_test_denominator)"
  expected_core="$(contract_value core_only_shell_tests)"
  expected_external="$(contract_value external_current_shell_tests)"

  if [[ -n "${ZMIN_UPSTREAM_GIT_TAG:-}" && "$ZMIN_UPSTREAM_GIT_TAG" != "$contract_tag" ]]; then
    echo "contract-check requires ZMIN_UPSTREAM_GIT_TAG=$contract_tag" >&2
    return 2
  fi
  if [[ "$contract_policy" != "archive_sha256_exact; tag_commit_declared; source_manifest_exact; no_checkout_fallback" ]]; then
    echo "unsupported source identity policy in contract: $contract_policy" >&2
    return 2
  fi
  source_identity_perl="${ZMIN_HTTP_PERL:-${ZMIN_TEST_PERL:-$(command -v perl 2>/dev/null || true)}}"
  ZMIN_UPSTREAM_GIT_CACHE="$cache_root" ZMIN_HTTP_PERL="$source_identity_perl" \
    "$repo_root/tools/git-upstream-http-provenance.sh" validate-source >/dev/null || {
    echo "strict upstream source identity verification failed" >&2
    return 1
  }
  if [[ -z "$contract_repo" ]]; then
    echo "missing upstream_git_repo in contract" >&2
    return 2
  fi
  if [[ "$contract_repo" != "${contract_archive_url%/archive/*}.git" ]]; then
    echo "upstream repository does not match archive URL" >&2
    return 1
  fi
  if [[ "$contract_mode" != "all-nondeprecated" ]]; then
    echo "unsupported authoritative manifest in contract: $contract_mode" >&2
    return 2
  fi
  if ! legacy_audit >/dev/null; then
    echo "legacy exclusion count mismatch" >&2
    return 1
  fi

  local invalid_classification
  invalid_classification="$(awk -F '\t' 'NR > 1 && $5 != "upstream deprecated/removed" && $5 != "external-but-current" { print $1; exit }' "$legacy_excludes")"
  if [[ -n "$invalid_classification" ]]; then
    echo "unsupported exclusion classification: $invalid_classification" >&2
    return 1
  fi

  normalize_group_list() {
    printf '%s\n' "$1" |
      tr ';' '\n' |
      sed '/^$/d' |
      LC_ALL=C sort |
      awk 'NR > 1 { printf ";" } { printf "%s", $0 } END { print "" }'
  }
  actual_group_list() {
    awk -F '\t' -v classification="$1" \
      'NR > 1 && $5 == classification { print $1 "|" $2 "|" $3 }' \
      "$legacy_excludes" |
      LC_ALL=C sort |
      awk 'NR > 1 { printf ";" } { printf "%s", $0 } END { print "" }'
  }
  if [[ "$(normalize_group_list "$expected_deprecated_groups")" != "$(actual_group_list 'upstream deprecated/removed')" ]]; then
    echo "deprecated exclusion groups drift from contract" >&2
    return 1
  fi
  if [[ "$(normalize_group_list "$expected_external_groups")" != "$(actual_group_list 'external-but-current')" ]]; then
    echo "external-current exclusion groups drift from contract" >&2
    return 1
  fi
  local invalid_scope_or_evidence
  invalid_scope_or_evidence="$(awk -F '\t' '
    NR == 1 { next }
    $5 == "upstream deprecated/removed" && $6 != "exclude from current nondeprecated scope" { print $1; next }
    $5 == "external-but-current" && $6 != "exclude from core-only suite; include in current contract" { print $1; next }
    ($5 == "upstream deprecated/removed" || $5 == "external-but-current") &&
      ($7 !~ /Documentation\// || $7 !~ /t\// || $7 !~ /command-list\.txt/) { print $1; next }
  ' "$legacy_excludes")"
  if [[ -n "$invalid_scope_or_evidence" ]]; then
    echo "exclusion scope/evidence drift: $invalid_scope_or_evidence" >&2
    return 1
  fi

  source_dir_for_contract="$cache_root/git-$contract_tag"
  archive="$cache_root/$contract_tag.tar.gz"
  if [[ ! -d "$source_dir_for_contract/t" || ! -s "$archive" ]]; then
    echo "contract source or archive missing under $cache_root" >&2
    return 2
  fi
  actual_sha="$(shasum -a 256 "$archive" | awk '{ print $1 }')"
  if [[ "$actual_sha" != "$contract_sha" ]]; then
    echo "upstream archive SHA-256 mismatch: expected $contract_sha, got $actual_sha" >&2
    return 1
  fi
  if [[ -d "$source_dir_for_contract/.git" ]]; then
    local actual_commit
    actual_commit="$(git -C "$source_dir_for_contract" rev-parse HEAD)"
    if [[ "$actual_commit" != "$contract_commit" ]]; then
      echo "upstream source commit mismatch: expected $contract_commit, got $actual_commit" >&2
      return 1
    fi
  fi

  local actual_total actual_denominator actual_core deprecated_count external_count
  actual_total="$(find "$source_dir_for_contract/t" -maxdepth 1 -type f -name 't[0-9][0-9][0-9][0-9]-*.sh' -print | wc -l | tr -d ' ')"
  actual_denominator="$(ZMIN_UPSTREAM_GIT_CACHE="$cache_root" "$repo_root/tools/git-upstream-compat-manifest.sh" all-nondeprecated | awk -F '\t' 'NR > 1 { count += 1 } END { print count + 0 }')"
  actual_core="$(ZMIN_UPSTREAM_GIT_CACHE="$cache_root" "$repo_root/tools/git-upstream-compat-manifest.sh" full-core | awk -F '\t' 'NR > 1 { count += 1 } END { print count + 0 }')"
  deprecated_count="$(awk -F '\t' 'NR > 1 && $5 == "upstream deprecated/removed" { sum += $3 } END { print sum + 0 }' "$legacy_excludes")"
  external_count="$(awk -F '\t' 'NR > 1 && $5 == "external-but-current" { sum += $3 } END { print sum + 0 }' "$legacy_excludes")"

  if [[ "$actual_total" != "$expected_total" || "$actual_denominator" != "$expected_denominator" ||
    "$actual_core" != "$expected_core" || "$deprecated_count" != "$expected_deprecated" ||
    "$external_count" != "$expected_external" ]]; then
    printf 'contract count mismatch: total=%s/%s denominator=%s/%s core=%s/%s deprecated=%s/%s external=%s/%s\n' \
      "$actual_total" "$expected_total" "$actual_denominator" "$expected_denominator" \
      "$actual_core" "$expected_core" "$deprecated_count" "$expected_deprecated" \
      "$external_count" "$expected_external" >&2
    return 1
  fi

  printf 'contract\t%s\tcommit=%s\tarchive_sha256=%s\tmanifest=%s\tdenominator=%s\n' \
    "$contract_tag" "$contract_commit" "$contract_sha" "$contract_mode" "$actual_denominator"
}

case "$mode" in
  family-summary)
    family_summary
    ;;
  legacy-audit)
    legacy_audit
    ;;
  contract-check)
    contract_check
    ;;
esac
