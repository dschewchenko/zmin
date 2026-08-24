#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
gate="$repo_root/tools/git-upstream-compat-contract-gate.sh"
contract="$repo_root/tools/git-upstream-compat-contract.tsv"
extension_contract="$repo_root/tools/zmin-extensions-contract.tsv"
cache_root="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/zmin-contract-gate-test.XXXXXX")"
cleanup_fixture_root() {
  chmod -R u+w "$fixture_root" 2>/dev/null || true
  rm -rf "$fixture_root"
}
trap cleanup_fixture_root EXIT

run_neutral() {
  (cd /tmp && "$@")
}

contract_value() {
  awk -F '\t' -v key="$1" '$1 == key { print $2; exit }' "$contract"
}

test_name_digest() {
  awk -F '\t' 'NR > 1 { print $2 }' "$1" | shasum -a 256 | awk '{ print $1 }'
}

expect_failure() {
  local name="$1"
  local diagnostic="$2"
  shift 2
  local output="$fixture_root/$name.out"
  set +e
  "$@" >"$output" 2>&1
  local rc=$?
  set -e
  if [[ "$rc" == "0" ]]; then
    echo "expected failure did not occur: $name" >&2
    cat "$output" >&2
    exit 1
  fi
  if ! grep -Fq -- "$diagnostic" "$output"; then
    echo "expected diagnostic did not occur: $name: $diagnostic" >&2
    cat "$output" >&2
    exit 1
  fi
  printf 'expected-failure\t%s\trc=%s\n' "$name" "$rc"
}

contract_gate() {
  run_neutral env \
    ZMIN_UPSTREAM_COMPAT_GATE_TESTING=1 \
    ZMIN_UPSTREAM_COMPAT_CONTRACT="$1" \
    ZMIN_EXTENSIONS_CONTRACT="${2:-$extension_contract}" \
    ZMIN_UPSTREAM_GIT_CACHE="$cache_root" \
    "$gate" check
}

contract_gate_with_cache() {
  run_neutral env \
    ZMIN_UPSTREAM_COMPAT_GATE_TESTING=1 \
    ZMIN_UPSTREAM_COMPAT_CONTRACT="$1" \
    ZMIN_EXTENSIONS_CONTRACT="$2" \
    ZMIN_UPSTREAM_GIT_CACHE="$3" \
    "$gate" check
}

run_neutral "$gate" check >"$fixture_root/valid.out"
grep -qx 'contract_gate=pass' "$fixture_root/valid.out"
grep -qx $'contract\tv2.55.0\tcommit=e9019fcafe0040228b8631c30f97ae1adb61bcdc\tarchive_sha256=72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49\tmanifest=all-nondeprecated\tdenominator=1045' "$fixture_root/valid.out"
grep -qx $'zmin_extension_contract=pass\tprimary=40\trelationships=7' "$fixture_root/valid.out"
grep -qx 'compatibility_claim=unverified' "$fixture_root/valid.out"
echo 'valid contract gate passed'

production_gate_with_override() {
  local override_name="$1"
  local override_value="$2"
  run_neutral env \
    -u ZMIN_UPSTREAM_COMPAT_GATE_TESTING \
    -u ZMIN_UPSTREAM_COMPAT_REPO_ROOT \
    -u ZMIN_UPSTREAM_COMPAT_CONTRACT \
    -u ZMIN_EXTENSIONS_CONTRACT \
    "$override_name=$override_value" \
    ZMIN_UPSTREAM_GIT_CACHE="$cache_root" \
    "$gate" check
}

expect_failure production-repo-root-override \
  'production invocation forbids test-only override: ZMIN_UPSTREAM_COMPAT_REPO_ROOT' \
  production_gate_with_override ZMIN_UPSTREAM_COMPAT_REPO_ROOT "$fixture_root/redirected-repo"
expect_failure production-contract-override \
  'production invocation forbids test-only override: ZMIN_UPSTREAM_COMPAT_CONTRACT' \
  production_gate_with_override ZMIN_UPSTREAM_COMPAT_CONTRACT "$fixture_root/redirected-contract.tsv"
expect_failure production-extension-override \
  'production invocation forbids test-only override: ZMIN_EXTENSIONS_CONTRACT' \
  production_gate_with_override ZMIN_EXTENSIONS_CONTRACT "$fixture_root/redirected-extensions.tsv"

tag="$(contract_value upstream_git_tag)"
bad_archive_cache="$fixture_root/cache-bad-archive"
mkdir -p "$bad_archive_cache"
cp "$cache_root/$tag.tar.gz" "$bad_archive_cache/$tag.tar.gz"
ln -s "$cache_root/git-$tag" "$bad_archive_cache/git-$tag"
printf 'mutated archive fixture\n' >>"$bad_archive_cache/$tag.tar.gz"
expect_failure cached-archive-sha 'cached archive SHA-256 mismatch' \
  contract_gate_with_cache "$contract" "$extension_contract" "$bad_archive_cache"

bad_checkout_cache="$fixture_root/cache-bad-checkout"
mkdir -p "$bad_checkout_cache/checkout-$tag"
cp "$cache_root/$tag.tar.gz" "$bad_checkout_cache/$tag.tar.gz"
ln -s "$cache_root/git-$tag" "$bad_checkout_cache/git-$tag"
bad_checkout_git="$bad_checkout_cache/checkout-$tag/.git"
(cd /tmp && env GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  git init --bare -q "$bad_checkout_git")
empty_tree="$(cd /tmp && env GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  git --git-dir="$bad_checkout_git" mktree </dev/null)"
wrong_checkout_commit="$(cd /tmp && printf 'wrong checkout\n' | env \
  GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  GIT_AUTHOR_NAME=fixture GIT_AUTHOR_EMAIL=fixture@example.invalid \
  GIT_COMMITTER_NAME=fixture GIT_COMMITTER_EMAIL=fixture@example.invalid \
  git --git-dir="$bad_checkout_git" commit-tree "$empty_tree" -F -)"
(cd /tmp && env GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
  git --git-dir="$bad_checkout_git" update-ref refs/heads/main "$wrong_checkout_commit")
printf 'ref: refs/heads/main\n' >"$bad_checkout_git/HEAD"
expect_failure cached-checkout-head 'optional checkout HEAD mismatch' \
  contract_gate_with_cache "$contract" "$extension_contract" "$bad_checkout_cache"

drifted_contract="$fixture_root/drifted-contract.tsv"
cp "$contract" "$drifted_contract"
perl -0pi -e 's/^authoritative_upstream_test_denominator\t1045$/authoritative_upstream_test_denominator\t1044/m' "$drifted_contract"
expect_failure denominator-drift 'authoritative denominator drift' contract_gate "$drifted_contract"

for drift_key in upstream_git_tag upstream_git_repo upstream_git_commit upstream_archive_sha256; do
  identity_contract="$fixture_root/$drift_key.tsv"
  cp "$contract" "$identity_contract"
  perl -0pi -e "s/^${drift_key}\\t[^\\n]*$/${drift_key}\\tDRIFT/m" "$identity_contract"
  case "$drift_key" in
    upstream_git_tag) diagnostic='upstream tag drift' ;;
    upstream_git_repo) diagnostic='upstream repository drift' ;;
    upstream_git_commit) diagnostic='upstream commit drift' ;;
    upstream_archive_sha256) diagnostic='upstream archive SHA drift' ;;
  esac
  expect_failure "identity-$drift_key" "$diagnostic" contract_gate "$identity_contract"
done

mutate_contract_claim() {
  local name="$1"
  local key="$2"
  local value="$3"
  local diagnostic="$4"
  local mutated="$fixture_root/$name.tsv"
  cp "$contract" "$mutated"
  perl -0pi -e "s{^${key}\t[^\n]*$}{${key}\t${value}}m" "$mutated"
  expect_failure "$name" "$diagnostic" contract_gate "$mutated"
}

mutate_contract_claim source-policy-drift source_identity_policy 'wrong_policy' 'source identity policy drift'
mutate_contract_claim archive-url-drift upstream_archive_url 'https://example.invalid/v2.55.0.tar.gz' 'archive URL drift'
mutate_contract_claim deprecated-group-drift deprecated_removed_groups 'wrong|group|1' 'deprecated exclusion group drift'
mutate_contract_claim external-group-drift external_current_groups 'wrong|groups' 'external-current exclusion group drift'
mutate_contract_claim total-drift upstream_top_level_shell_tests 1045 'upstream total drift'
mutate_contract_claim excluded-count-drift excluded_deprecated_removed_shell_tests 2 'deprecated total drift'
mutate_contract_claim core-manifest-drift core_only_manifest all-nondeprecated 'core-only manifest drift'
mutate_contract_claim core-count-drift core_only_shell_tests 927 'core-only count drift'
mutate_contract_claim external-count-drift external_current_shell_tests 116 'external-current count drift'
mutate_contract_claim required-evidence-drift required_evidence 'upstream shell suite' 'required evidence drift'
mutate_contract_claim current-count-drift current_git_non_extension_count 6 'current Git non-extension count drift'
mutate_contract_claim current-source-drift current_git_non_extension_backfill 'backfill|Documentation/missing.adoc|t/t5620-backfill.sh|current-git-v2.55.0' 'current Git non-extension metadata drift: current_git_non_extension_backfill'

extension_gate() {
  run_neutral env \
    ZMIN_UPSTREAM_COMPAT_GATE_TESTING=1 \
    ZMIN_UPSTREAM_COMPAT_CONTRACT="$contract" \
    ZMIN_EXTENSIONS_CONTRACT="$1" \
    ZMIN_UPSTREAM_GIT_CACHE="$cache_root" \
    "$gate" check
}

extension_gate_with_gate() {
  run_neutral env \
    ZMIN_UPSTREAM_COMPAT_GATE_TESTING=1 \
    ZMIN_UPSTREAM_COMPAT_REPO_ROOT="$repo_root" \
    ZMIN_UPSTREAM_COMPAT_CONTRACT="$contract" \
    ZMIN_EXTENSIONS_CONTRACT="$2" \
    ZMIN_UPSTREAM_GIT_CACHE="$cache_root" \
    "$1" check
}

extension_count_drift="$fixture_root/extension-count-drift.tsv"
cp "$extension_contract" "$extension_count_drift"
printf 'primary\tcommand.extra\tcommand\t-\textra\tcrates/zmin-cli-schema/src/lib.rs\tstable\tExtra {\tschema.command\n' >>"$extension_count_drift"
expect_failure extension-count-drift 'Zmin primary extension count drift' extension_gate "$extension_count_drift"

extension_duplicate="$fixture_root/extension-duplicate.tsv"
cp "$extension_contract" "$extension_duplicate"
sed -n '2p' "$extension_contract" >>"$extension_duplicate"
expect_failure extension-duplicate 'duplicate Zmin extension row: command.hooks' extension_gate "$extension_duplicate"

extension_missing="$fixture_root/extension-missing.tsv"
grep -v $'^primary\tcommand.lfs\t' "$extension_contract" >"$extension_missing"
expect_failure extension-missing 'Zmin extension required row missing: command.lfs' extension_gate "$extension_missing"

extension_invalid_kind="$fixture_root/extension-invalid-kind.tsv"
cp "$extension_contract" "$extension_invalid_kind"
perl -0pi -e 's/^primary\tcommand\.hooks\tcommand\t/primary\tcommand.hooks\tinvalid-kind\t/m' "$extension_invalid_kind"
expect_failure extension-invalid-kind 'invalid Zmin extension kind/parent/status/surface' extension_gate "$extension_invalid_kind"

extension_invalid_status="$fixture_root/extension-invalid-status.tsv"
cp "$extension_contract" "$extension_invalid_status"
perl -0pi -e 's/^primary\tcommand\.hooks\tcommand\t-\thooks\t([^\t]*)\tstable\t([^\n]*)$/primary\tcommand.hooks\tcommand\t-\thooks\t$1\timplemented\t$2/m' "$extension_invalid_status"
expect_failure extension-invalid-status 'invalid Zmin extension kind/parent/status/surface' extension_gate "$extension_invalid_status"

extension_lfs_deferred="$fixture_root/extension-lfs-deferred.tsv"
cp "$extension_contract" "$extension_lfs_deferred"
perl -0pi -e 's/^(primary\tcommand\.lfs\t[^\n]*\t)stable(\t)/${1}deferred$2/m' "$extension_lfs_deferred"
expect_failure extension-lfs-deferred 'invalid Zmin extension kind/parent/status/surface' extension_gate "$extension_lfs_deferred"

extension_broken_parent="$fixture_root/extension-broken-parent.tsv"
cp "$extension_contract" "$extension_broken_parent"
perl -0pi -e 's/^relationship\trelationship\.hooks\.init\trelationship\thooks\t/relationship\trelationship.hooks.init\trelationship\trepo\t/m' "$extension_broken_parent"
expect_failure extension-broken-parent 'invalid Zmin extension kind/parent/status/surface' extension_gate "$extension_broken_parent"

extension_misclassified_current="$fixture_root/extension-misclassified-current.tsv"
cp "$extension_contract" "$extension_misclassified_current"
perl -0pi -e 's/^primary\tcommand\.lfs\tcommand\t-\tlfs\t/primary\tcommand.lfs\tcommand\t-\trepo\t/m' "$extension_misclassified_current"
expect_failure extension-misclassified-current 'invalid Zmin extension kind/parent/status/surface' extension_gate "$extension_misclassified_current"

extension_irrelevant_token="$fixture_root/extension-irrelevant-token.tsv"
cp "$extension_contract" "$extension_irrelevant_token"
perl -0pi -e 's/\tHooks \{/\tpub/' "$extension_irrelevant_token"
expect_failure extension-irrelevant-token 'extension evidence anchor drift: command.hooks' extension_gate "$extension_irrelevant_token"

extension_irrelevant_path="$fixture_root/extension-irrelevant-path.tsv"
cp "$extension_contract" "$extension_irrelevant_path"
perl -0pi -e 's#(^primary\tcommand\.hooks\t[^\n]*?)crates/zmin-cli-schema/src/lib\.rs#$1crates/zmin-cli/src/cli/commands/lfs.rs#m' "$extension_irrelevant_path"
expect_failure extension-irrelevant-path 'extension scope evidence file drift: command.hooks' extension_gate "$extension_irrelevant_path"

extension_instaweb_wrong_scope="$fixture_root/extension-instaweb-wrong-scope.tsv"
cp "$extension_contract" "$extension_instaweb_wrong_scope"
perl -0pi -e 's/^(primary\toption\.instaweb\.daemon-internal[^\n]*\t)schema\.instaweb$/${1}schema.command/m' "$extension_instaweb_wrong_scope"
expect_failure extension-instaweb-wrong-scope 'extension scope drift: option.instaweb.daemon-internal' \
  extension_gate "$extension_instaweb_wrong_scope"

extension_cat_file_wrong_scope="$fixture_root/extension-cat-file-wrong-scope.tsv"
cp "$extension_contract" "$extension_cat_file_wrong_scope"
perl -0pi -e 's/^(primary\toption\.cat-file\.type[^\n]*\t)schema\.cat-file$/${1}schema.command/m' "$extension_cat_file_wrong_scope"
expect_failure extension-cat-file-wrong-scope 'extension scope drift: option.cat-file.type' \
  extension_gate "$extension_cat_file_wrong_scope"

extension_hook_wrong_scope="$fixture_root/extension-hook-wrong-scope.tsv"
cp "$extension_contract" "$extension_hook_wrong_scope"
perl -0pi -e 's/^(primary\thook-option\.hooks-add\.force[^\n]*\t)schema\.hooks-add$/${1}schema.command/m' "$extension_hook_wrong_scope"
expect_failure extension-hook-wrong-scope 'extension scope drift: hook-option.hooks-add.force' \
  extension_gate "$extension_hook_wrong_scope"

extension_hook_run_wrong_scope="$fixture_root/extension-hook-run-wrong-scope.tsv"
cp "$extension_contract" "$extension_hook_run_wrong_scope"
perl -0pi -e 's{^(primary\thook-option\.hooks-run\.ext[^\n]*\t)schema\.hooks-run$}{$1schema.command}m' "$extension_hook_run_wrong_scope"
expect_failure extension-hook-run-wrong-scope 'extension scope drift: hook-option.hooks-run.ext' \
  extension_gate "$extension_hook_run_wrong_scope"

extension_credential_wrong_file="$fixture_root/extension-credential-wrong-file.tsv"
cp "$extension_contract" "$extension_credential_wrong_file"
perl -0pi -e 's#crates/zmin-cli/src/cli/commands/credential_impl\.rs;crates/zmin-cli-schema/src/lib\.rs#crates/zmin-cli/src/cli/commands/admin_impl.rs;crates/zmin-cli-schema/src/lib.rs#' "$extension_credential_wrong_file"
expect_failure extension-credential-wrong-file 'extension scope evidence file drift: option.credential-cache.daemon-internal' \
  extension_gate "$extension_credential_wrong_file"

extension_credential_wrong_scope="$fixture_root/extension-credential-wrong-scope.tsv"
cp "$extension_contract" "$extension_credential_wrong_scope"
perl -0pi -e 's/\truntime\.credential-cache$/\tschema.command/m' "$extension_credential_wrong_scope"
expect_failure extension-credential-wrong-scope 'extension scope drift: option.credential-cache.daemon-internal' \
  extension_gate "$extension_credential_wrong_scope"

extension_anchor_outside_scope_gate="$fixture_root/extension-anchor-outside-scope-gate.sh"
cp "$gate" "$extension_anchor_outside_scope_gate"
perl -0pi -e "s/scope_start='    Instaweb \\{'/scope_start='    CredentialCache {'/" "$extension_anchor_outside_scope_gate"
expect_failure extension-anchor-outside-scope 'extension scope boundary/anchor failure: option.instaweb.daemon-internal' \
  extension_gate_with_gate "$extension_anchor_outside_scope_gate" "$extension_contract"

extension_missing_scope_end_gate="$fixture_root/extension-missing-scope-end-gate.sh"
cp "$gate" "$extension_missing_scope_end_gate"
perl -0pi -e "s/scope_end='pub enum RefsCommand \\{'/scope_end='pub enum MissingRefsCommand {'/" "$extension_missing_scope_end_gate"
expect_failure extension-missing-scope-end 'extension scope boundary/anchor failure: subcommand.hooks.init' \
  extension_gate_with_gate "$extension_missing_scope_end_gate" "$extension_contract"

extension_missing_scope_start_gate="$fixture_root/extension-missing-scope-start-gate.sh"
cp "$gate" "$extension_missing_scope_start_gate"
perl -0pi -e "s/scope_start='pub enum ManagedHooksCommand \\{'/scope_start='pub enum MissingManagedHooksCommand {'/" "$extension_missing_scope_start_gate"
expect_failure extension-missing-scope-start 'extension scope boundary/anchor failure: subcommand.hooks.init' \
  extension_gate_with_gate "$extension_missing_scope_start_gate" "$extension_contract"

version_drift_contract="$fixture_root/version-drift-contract.tsv"
cp "$contract" "$version_drift_contract"
perl -0pi -e 's/current-git-v2\.55\.0/current-git-v2\.54\.0/' "$version_drift_contract"
expect_failure current-source-version-drift 'current Git non-extension metadata drift: current_git_non_extension_backfill' contract_gate "$version_drift_contract"

source_path_drift_contract="$fixture_root/source-path-drift-contract.tsv"
cp "$contract" "$source_path_drift_contract"
perl -0pi -e 's#Documentation/git-backfill\.adoc#Documentation/missing-backfill.adoc#' "$source_path_drift_contract"
expect_failure current-source-path-drift 'current Git non-extension metadata drift: current_git_non_extension_backfill' contract_gate "$source_path_drift_contract"

tag="$(contract_value upstream_git_tag)"
commit="$(contract_value upstream_git_commit)"
archive_sha="$(contract_value upstream_archive_sha256)"
denominator="$(contract_value authoritative_upstream_test_denominator)"

exploratory_manifest="$fixture_root/exploratory-manifest.tsv"
exploratory_summary="$fixture_root/exploratory-summary.tsv"
printf 'mode\ttest\treason\nall-nondeprecated\tt0000-basic.sh\tfixture\n' >"$exploratory_manifest"
printf 'mode\ttest\tstatus\treason\tlog\nall-nondeprecated\tt0000-basic.sh\tpass\tfixture\tfixture.log\n' >"$exploratory_summary"
exploratory_metadata="$fixture_root/exploratory.tsv"
{
  printf 'key\tvalue\n'
  printf 'upstream_git_tag\t%s\n' "$tag"
  printf 'upstream_git_commit\t%s\n' "$commit"
  printf 'upstream_archive_sha256\t%s\n' "$archive_sha"
  printf 'mode\tall-nondeprecated\n'
  printf 'scope\texploratory-bounded\n'
  printf 'evidence_scope\texploratory\n'
  printf 'compatibility_claim\tunverified\n'
  printf 'manifest_file\t%s\n' "$exploratory_manifest"
  printf 'manifest_sha256\t%s\n' "$(test_name_digest "$exploratory_manifest")"
  printf 'manifest_test_count\t1\n'
  printf 'summary_file\t%s\n' "$exploratory_summary"
  printf 'summary_sha256\t%s\n' "$(shasum -a 256 "$exploratory_summary" | awk '{ print $1 }')"
  printf 'manifest_offset\t0\nmanifest_limit\t1\ntotal\t1\npassed\t1\nfailed\t0\n'
} >"$exploratory_metadata"
run_neutral "$gate" validate-run "$exploratory_metadata" >"$fixture_root/exploratory.out"
grep -qx 'run_scope=exploratory-or-incomplete' "$fixture_root/exploratory.out"
grep -qx 'compatibility_claim=unverified' "$fixture_root/exploratory.out"
expect_failure exploratory-claim 'invalid authoritative manifest header' run_neutral "$gate" validate-run "$exploratory_metadata" --require-authoritative

authoritative_metadata="$fixture_root/authoritative.tsv"
authoritative_manifest="$fixture_root/authoritative-manifest.tsv"
authoritative_summary="$fixture_root/authoritative-summary.tsv"
{
  printf '# mode\ttest\treason\n'
  for source_file in "$cache_root/git-$tag"/t/t[0-9][0-9][0-9][0-9]-*.sh; do
    [[ -f "$source_file" ]] || continue
    test_name="${source_file##*/}"
    [[ "$test_name" == "t5323-pack-redundant.sh" ]] && continue
    printf 'all-nondeprecated\t%s\tcomplete upstream top-level shell suite minus explicit whole-file deprecated excludes\n' "$test_name"
  done
} >"$authoritative_manifest"
{
  printf 'mode\ttest\tstatus\treason\tlog\n'
  awk -F '\t' 'NR > 1 { printf "%s\t%s\tpass\tfixture\tfixture.log\n", $1, $2 }' "$authoritative_manifest"
} >"$authoritative_summary"
{
  printf 'key\tvalue\n'
  printf 'upstream_git_tag\t%s\n' "$tag"
  printf 'upstream_git_commit\t%s\n' "$commit"
  printf 'upstream_archive_sha256\t%s\n' "$archive_sha"
  printf 'mode\tall-nondeprecated\n'
  printf 'scope\tcurrent-contract\n'
  printf 'evidence_scope\tauthoritative-upstream-suite\n'
  printf 'compatibility_claim\tunverified\n'
  printf 'manifest_file\t%s\n' "$authoritative_manifest"
  printf 'manifest_sha256\t%s\n' "$(test_name_digest "$authoritative_manifest")"
  printf 'manifest_test_count\t%s\n' "$denominator"
  printf 'summary_file\t%s\n' "$authoritative_summary"
  printf 'summary_sha256\t%s\n' "$(shasum -a 256 "$authoritative_summary" | awk '{ print $1 }')"
  printf 'manifest_offset\t0\nmanifest_limit\t0\ntotal\t%s\npassed\t%s\nfailed\t0\n' "$denominator" "$denominator"
} >"$authoritative_metadata"
run_neutral "$gate" validate-run "$authoritative_metadata" --require-authoritative >"$fixture_root/authoritative.out"
grep -qx 'run_scope=authoritative-upstream-suite' "$fixture_root/authoritative.out"
grep -qx 'compatibility_claim=unverified' "$fixture_root/authoritative.out"
echo 'run evidence classification passed'

echo 'direct pinned source manifest validation passed; denominator=1045/1046'

prepare_source_archive="$cache_root/$tag.tar.gz"
[[ -f "$prepare_source_archive" && -s "$prepare_source_archive" ]] || {
  echo "fresh-cache fixture requires the local pinned archive: $prepare_source_archive" >&2
  exit 2
}
prepare_fixture="$fixture_root/prepare-cache"
prepare_bin="$prepare_fixture/bin"
mkdir -p "$prepare_bin"
prepare_curl_log="$prepare_fixture/curl.log"
printf '%s\n' \
  '#!/bin/sh' \
  'out=' \
  'previous=' \
  'for argument do' \
  '  if [ "$previous" = "-o" ]; then out="$argument"; fi' \
  '  previous="$argument"' \
  'done' \
  '[ -n "$out" ] || exit 64' \
  'printf "local-curl\n" >>"$ZMIN_GATE_CURL_LOG"' \
  'cp "$ZMIN_GATE_FIXTURE_ARCHIVE" "$out"' >"$prepare_bin/curl"
printf '%s\n' \
  '#!/bin/sh' \
  'if [ "$1" = "ls-remote" ]; then' \
  '  printf "%s\trefs/tags/v2.55.0^{}\n" e9019fcafe0040228b8631c30f97ae1adb61bcdc' \
  '  exit 0' \
  'fi' \
  'printf "unexpected git invocation\n" >&2' \
  'exit 127' >"$prepare_bin/git"
chmod +x "$prepare_bin/curl" "$prepare_bin/git"

run_offline_prepare() {
  local target_cache="$1"
  run_neutral env \
    PATH="$prepare_bin:$PATH" \
    ZMIN_GATE_FIXTURE_ARCHIVE="$prepare_source_archive" \
    ZMIN_GATE_CURL_LOG="$prepare_curl_log" \
    ZMIN_UPSTREAM_GIT_CACHE="$target_cache" \
    ZMIN_CONTRACT_CACHE_LOCK_TIMEOUT_SECONDS=20 \
    "$gate" prepare-and-check
}

clean_prepare_cache="$prepare_fixture/clean-cache"
run_offline_prepare "$clean_prepare_cache" >"$prepare_fixture/clean.out"
grep -qx 'contract_gate=pass' "$prepare_fixture/clean.out"
grep -qx "$archive_sha" "$clean_prepare_cache/git-$tag/.zmin-pristine-source.sha256"
[[ ! -w "$clean_prepare_cache/git-$tag" ]] || {
  echo 'fresh-cache source tree remained writable' >&2
  exit 1
}
[[ -z "$(find "$clean_prepare_cache/git-$tag" \( -type f -o -type d \) -perm -u+w -print -quit)" ]] || {
  echo 'fresh-cache source entry remained writable' >&2
  exit 1
}
[[ ! -e "$clean_prepare_cache/.zmin-upstream-contract.lock" ]] || {
  echo 'fresh-cache lock was not cleaned after success' >&2
  exit 1
}
echo 'fresh-cache marker/readonly validation passed'

concurrent_cache="$prepare_fixture/concurrent-cache"
set +e
run_offline_prepare "$concurrent_cache" >"$prepare_fixture/concurrent-1.out" 2>&1 &
prepare_pid_one=$!
run_offline_prepare "$concurrent_cache" >"$prepare_fixture/concurrent-2.out" 2>&1 &
prepare_pid_two=$!
wait "$prepare_pid_one"
prepare_status_one=$?
wait "$prepare_pid_two"
prepare_status_two=$?
set -e
[[ "$prepare_status_one" == "0" && "$prepare_status_two" == "0" ]] || {
  echo "concurrent prepare failed: $prepare_status_one/$prepare_status_two" >&2
  cat "$prepare_fixture/concurrent-1.out" >&2
  cat "$prepare_fixture/concurrent-2.out" >&2
  exit 1
}
[[ "$(wc -l <"$prepare_curl_log" | tr -d ' ')" == "2" ]] || {
  echo 'concurrent prepare did not perform exactly one additional archive fetch' >&2
  cat "$prepare_curl_log" >&2
  exit 1
}
[[ ! -e "$concurrent_cache/.zmin-upstream-contract.lock" ]] || {
  echo 'concurrent prepare left its cache lock behind' >&2
  exit 1
}
echo 'concurrent fresh-cache publication passed'

tampered_cache="$prepare_fixture/tampered-cache"
cp -R "$clean_prepare_cache" "$tampered_cache"
tampered_source="$tampered_cache/git-$tag/GIT-VERSION-GEN"
chmod u+w "$tampered_source"
printf '%s\n' '# fresh-cache tamper fixture' >>"$tampered_source"
chmod a-w "$tampered_source"
expect_failure fresh-cache-invalid-winner \
  'validated pristine source manifest mismatch' \
  run_offline_prepare "$tampered_cache"
[[ ! -e "$tampered_cache/.zmin-upstream-contract.lock" ]] || {
  echo 'tampered winner left its cache lock behind' >&2
  exit 1
}
echo 'fresh-cache invalid-winner rejection passed'
