#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: tools/git-upstream-compat-contract-gate.sh [check|prepare-and-check|validate-run]

Checks the canonical current-Git contract before any compatibility evidence is
used. The gate never claims full compatibility: it reports the scope contract
or classifies a suite metadata file as authoritative upstream evidence versus
exploratory/partial evidence.

Modes:
  check             require the exact pinned archive/source cache and verify it
  prepare-and-check download the exact contract archive, then run check
  validate-run FILE classify suite run-metadata.tsv; add
                    --require-authoritative to fail on partial/exploratory data

Environment:
  ZMIN_UPSTREAM_GIT_CACHE  Cache dir. Default: ~/.cache/zmin/git-upstream.
EOF
}

mode="${1:-check}"
case "$mode" in
  check|prepare-and-check)
    [[ "$#" == "1" ]] || { usage; exit 2; }
    ;;
  validate-run)
    [[ "$#" == "2" || "$#" == "3" ]] || { usage; exit 2; }
    metadata_file="$2"
    require_authoritative=0
    if [[ "$#" == "3" ]]; then
      [[ "$3" == "--require-authoritative" ]] || { usage; exit 2; }
      require_authoritative=1
    fi
    ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

testing_mode="${ZMIN_UPSTREAM_COMPAT_GATE_TESTING:-0}"
if [[ "$testing_mode" != "1" ]]; then
  if [[ "${ZMIN_UPSTREAM_COMPAT_REPO_ROOT+x}" == "x" ]]; then
    echo "production invocation forbids test-only override: ZMIN_UPSTREAM_COMPAT_REPO_ROOT" >&2
    exit 2
  fi
  if [[ "${ZMIN_UPSTREAM_COMPAT_CONTRACT+x}" == "x" ]]; then
    echo "production invocation forbids test-only override: ZMIN_UPSTREAM_COMPAT_CONTRACT" >&2
    exit 2
  fi
  if [[ "${ZMIN_EXTENSIONS_CONTRACT+x}" == "x" ]]; then
    echo "production invocation forbids test-only override: ZMIN_EXTENSIONS_CONTRACT" >&2
    exit 2
  fi
fi

repo_root="${ZMIN_UPSTREAM_COMPAT_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
contract_file="$repo_root/tools/git-upstream-compat-contract.tsv"
if [[ "$testing_mode" == "1" ]]; then
  contract_file="${ZMIN_UPSTREAM_COMPAT_CONTRACT:-$contract_file}"
fi
extension_contract_file="$repo_root/tools/zmin-extensions-contract.tsv"
if [[ "$testing_mode" == "1" ]]; then
  extension_contract_file="${ZMIN_EXTENSIONS_CONTRACT:-$extension_contract_file}"
fi
cache_root="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
contract_cache_lock_dir="$cache_root/.zmin-upstream-contract.lock"
contract_cache_lock_token=""
contract_cache_lock_held=0

# These are immutable CI sentinels. The TSV remains the human/machine-readable
# contract, while this gate prevents a simultaneous contract-and-cache drift
# from silently redefining the current-Git claim.
readonly EXPECTED_CONTRACT_ID="git-current-nondeprecated-v2.55.0"
readonly EXPECTED_TAG="v2.55.0"
readonly EXPECTED_REPO="https://github.com/git/git.git"
readonly EXPECTED_COMMIT="e9019fcafe0040228b8631c30f97ae1adb61bcdc"
readonly EXPECTED_SOURCE_IDENTITY_POLICY="archive_sha256_exact; tag_commit_declared; source_manifest_exact; no_checkout_fallback"
readonly EXPECTED_ARCHIVE_SHA="72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49"
readonly EXPECTED_ARCHIVE_URL="https://github.com/git/git/archive/refs/tags/v2.55.0.tar.gz"
readonly EXPECTED_MANIFEST="all-nondeprecated"
readonly EXPECTED_CORE_MANIFEST="full-core"
readonly EXPECTED_DEPRECATED_GROUPS="t5323-pack-redundant|t5323|1"
readonly EXPECTED_EXTERNAL_GROUPS="git-svn|t91|69;git-cvsserver|t94|3;gitweb|t95|3;cvsimport|t96|5;git-p4|t98[0-3]|37"
readonly EXPECTED_REQUIRED_EVIDENCE="upstream shell suite; stock Git differential; macOS; Linux; Windows"
readonly EXPECTED_TOTAL=1046
readonly EXPECTED_DEPRECATED=1
readonly EXPECTED_DENOMINATOR=1045
readonly EXPECTED_CORE=928
readonly EXPECTED_EXTERNAL=117
readonly EXPECTED_CURRENT_NON_EXTENSION_COUNT=7

current_git_non_extension_keys=(
  current_git_non_extension_backfill
  current_git_non_extension_diff_pairs
  current_git_non_extension_format_rev
  current_git_non_extension_history
  current_git_non_extension_last_modified
  current_git_non_extension_repo
  current_git_non_extension_url_parse
)
current_git_non_extension_values=(
  'backfill|Documentation/git-backfill.adoc|t/t5620-backfill.sh|current-git-v2.55.0'
  'diff-pairs|Documentation/git-diff-pairs.adoc|t/t4070-diff-pairs.sh|current-git-v2.55.0'
  'format-rev|Documentation/git-format-rev.adoc|t/t6120-describe.sh|current-git-v2.55.0'
  'history|Documentation/git-history.adoc|t/t3450-history.sh,t/t3451-history-reword.sh,t/t3452-history-split.sh,t/t3453-history-fixup.sh|current-git-v2.55.0'
  'last-modified|Documentation/git-last-modified.adoc|t/t8020-last-modified.sh|current-git-v2.55.0'
  'repo|Documentation/git-repo.adoc|t/t1900-repo-info.sh,t/t1901-repo-structure.sh|current-git-v2.55.0'
  'url-parse|Documentation/git-url-parse.adoc|t/t9904-url-parse.sh|current-git-v2.55.0'
)

expected_extension_primary_ids=(
  command.hooks command.save command.changes command.publish command.update
  command.undo command.timeline command.recover command.compatibility command.lfs
  subcommand.hooks.init subcommand.hooks.add subcommand.hooks.list
  subcommand.hooks.remove subcommand.hooks.run
  option.clone.worktree-first option.clone.instant option.clone.background-fetch
  option.clone.demand-hydrate option.cat-file.type option.cat-file.size
  option.cat-file.exists option.cat-file.pretty option.imap-send.folder
  option.imap-send.list option.imap-send.short-folder
  option.credential-cache.daemon-internal option.instaweb.daemon-internal
  option.instaweb.git-dir option.instaweb.work-tree
  hook-option.hook-run.ignore-missing hook-option.hook-run.to-stdin
  hook-option.hooks-add.force hook-option.hooks-add.staged-runner
  hook-option.hooks-add.ext hook-option.hooks-run.staged
  hook-option.hooks-run.ext hook-option.hooks-run.list hook-option.hooks-run.dry-run
  environment.ZMIN_GIT_HTTP_VERSION
)
expected_extension_relationship_ids=(
  relationship.hooks.init relationship.hooks.add relationship.hooks.list
  relationship.hooks.remove relationship.hooks.run relationship.repo.info
  relationship.repo.structure
)
expected_extension_primary_anchors=(
  'Hooks {' 'Save {' 'Changes,' 'Publish,' 'Update,' 'Undo,' 'Timeline,'
  'Recover {' 'Compatibility {' 'pub(crate) fn lfs_command(args: Vec<String>) -> Result<()> {'
  'Init,' 'Add {' 'List,' 'Remove {' 'Run {'
  'long = "worktree-first"' 'long = "instant"' 'long = "background-fetch"'
  'long = "demand-hydrate"' 'long = "type"' 'long = "size"' 'long = "exists"'
  'long = "pretty"' 'long = "folder"' 'long = "list"'
  'short = '\''f'\'', long = "folder"' 'if daemon_internal {'
  'long = "daemon-internal"' 'long = "git-dir"' 'long = "work-tree"'
  'long = "ignore-missing"' 'long = "to-stdin"' 'long = "force"'
  'long = "staged-runner"' 'long = "ext"' 'long = "staged"'
  'long = "ext"' 'long = "list"' 'long = "dry-run"'
  'std::env::var("ZMIN_GIT_HTTP_VERSION")'
)
expected_extension_relationship_anchors=(
  'Init,' 'Add {' 'List,' 'Remove {' 'Run {'
  'current_git_non_extension_repo' 'current_git_non_extension_repo'
)

contract_value() {
  local key="$1"
  awk -F '\t' -v key="$key" '$1 == key { print $2; exit }' "$contract_file"
}

required_keys=(
  contract_id
  upstream_git_tag
  upstream_git_repo
  upstream_git_commit
  source_identity_policy
  upstream_archive_url
  upstream_archive_sha256
  authoritative_manifest
  deprecated_removed_groups
  external_current_groups
  upstream_top_level_shell_tests
  excluded_deprecated_removed_shell_tests
  authoritative_upstream_test_denominator
  core_only_manifest
  core_only_shell_tests
  external_current_shell_tests
  required_evidence
  current_git_non_extension_count
  current_git_non_extension_backfill
  current_git_non_extension_diff_pairs
  current_git_non_extension_format_rev
  current_git_non_extension_history
  current_git_non_extension_last_modified
  current_git_non_extension_repo
  current_git_non_extension_url_parse
)

validate_contract_shape() {
  [[ -f "$contract_file" ]] || { echo "missing compatibility contract: $contract_file" >&2; return 2; }
  local key count value
  for key in "${required_keys[@]}"; do
    count="$(awk -F '\t' -v key="$key" '$1 == key { count += 1 } END { print count + 0 }' "$contract_file")"
    value="$(contract_value "$key")"
    if [[ "$count" != "1" || -z "$value" ]]; then
      echo "contract key must occur exactly once with a value: $key" >&2
      return 1
    fi
  done

  [[ "$(contract_value contract_id)" == "$EXPECTED_CONTRACT_ID" ]] || { echo "contract id drift" >&2; return 1; }
  [[ "$(contract_value upstream_git_tag)" == "$EXPECTED_TAG" ]] || { echo "upstream tag drift" >&2; return 1; }
  [[ "$(contract_value upstream_git_repo)" == "$EXPECTED_REPO" ]] || { echo "upstream repository drift" >&2; return 1; }
  [[ "$(contract_value upstream_git_commit)" == "$EXPECTED_COMMIT" ]] || { echo "upstream commit drift" >&2; return 1; }
  [[ "$(contract_value source_identity_policy)" == "$EXPECTED_SOURCE_IDENTITY_POLICY" ]] || { echo "source identity policy drift" >&2; return 1; }
  [[ "$(contract_value upstream_archive_url)" == "$EXPECTED_ARCHIVE_URL" ]] || { echo "archive URL drift" >&2; return 1; }
  [[ "$(contract_value upstream_archive_sha256)" == "$EXPECTED_ARCHIVE_SHA" ]] || { echo "upstream archive SHA drift" >&2; return 1; }
  [[ "$(contract_value authoritative_manifest)" == "$EXPECTED_MANIFEST" ]] || { echo "authoritative manifest drift" >&2; return 1; }
  [[ "$(contract_value deprecated_removed_groups)" == "$EXPECTED_DEPRECATED_GROUPS" ]] || { echo "deprecated exclusion group drift" >&2; return 1; }
  [[ "$(contract_value external_current_groups)" == "$EXPECTED_EXTERNAL_GROUPS" ]] || { echo "external-current exclusion group drift" >&2; return 1; }
  [[ "$(contract_value upstream_top_level_shell_tests)" == "$EXPECTED_TOTAL" ]] || { echo "upstream total drift" >&2; return 1; }
  [[ "$(contract_value excluded_deprecated_removed_shell_tests)" == "$EXPECTED_DEPRECATED" ]] || { echo "deprecated total drift" >&2; return 1; }
  [[ "$(contract_value authoritative_upstream_test_denominator)" == "$EXPECTED_DENOMINATOR" ]] || { echo "authoritative denominator drift" >&2; return 1; }
  [[ "$(contract_value core_only_manifest)" == "$EXPECTED_CORE_MANIFEST" ]] || { echo "core-only manifest drift" >&2; return 1; }
  [[ "$(contract_value core_only_shell_tests)" == "$EXPECTED_CORE" ]] || { echo "core-only count drift" >&2; return 1; }
  [[ "$(contract_value external_current_shell_tests)" == "$EXPECTED_EXTERNAL" ]] || { echo "external-current count drift" >&2; return 1; }
  [[ "$(contract_value required_evidence)" == "$EXPECTED_REQUIRED_EVIDENCE" ]] || { echo "required evidence drift" >&2; return 1; }
  [[ "$(contract_value current_git_non_extension_count)" == "$EXPECTED_CURRENT_NON_EXTENSION_COUNT" ]] || {
    echo "current Git non-extension count drift" >&2
    return 1
  }
  local index current_key
  for index in "${!current_git_non_extension_keys[@]}"; do
    current_key="${current_git_non_extension_keys[$index]}"
    [[ "$(contract_value "$current_key")" == "${current_git_non_extension_values[$index]}" ]] || {
      echo "current Git non-extension metadata drift: $current_key" >&2
      return 1
    }
  done
  validate_extension_contract
}

extension_expected_scope() {
  local extension_id="$1"
  case "$extension_id" in
    command.lfs) printf '%s\n' 'runtime.lfs-command' ;;
    command.*) printf '%s\n' 'schema.command' ;;
    subcommand.hooks.*|relationship.hooks.*) printf '%s\n' 'schema.managed-hooks' ;;
    hook-option.hooks-add.*) printf '%s\n' 'schema.hooks-add' ;;
    hook-option.hooks-run.*) printf '%s\n' 'schema.hooks-run' ;;
    hook-option.hook-run.*) printf '%s\n' 'schema.hook-run' ;;
    option.clone.*) printf '%s\n' 'schema.clone' ;;
    option.cat-file.*) printf '%s\n' 'schema.cat-file' ;;
    option.imap-send.*) printf '%s\n' 'schema.imap-send' ;;
    option.credential-cache.*) printf '%s\n' 'runtime.credential-cache' ;;
    option.instaweb.*) printf '%s\n' 'schema.instaweb' ;;
    environment.*) printf '%s\n' 'runtime.http-version' ;;
    relationship.repo.*) printf '%s\n' 'contract.current-git-repo' ;;
    *) return 1 ;;
  esac
}

scope_definition() {
  local scope_id="$1"
  scope_same_line=0
  case "$scope_id" in
    schema.command)
      scope_file_rel='crates/zmin-cli-schema/src/lib.rs'
      scope_start='pub enum Command {'
      scope_end='pub enum HookCommand {'
      ;;
    schema.clone)
      scope_file_rel='crates/zmin-cli-schema/src/lib.rs'
      scope_start='    Clone {'
      scope_end='    HashObject {'
      ;;
    schema.cat-file)
      scope_file_rel='crates/zmin-cli-schema/src/lib.rs'
      scope_start='    CatFile {'
      scope_end='    GetTarCommitId,'
      ;;
    schema.imap-send)
      scope_file_rel='crates/zmin-cli-schema/src/lib.rs'
      scope_start='    ImapSend {'
      scope_end='    FilterBranch {'
      ;;
    schema.instaweb)
      scope_file_rel='crates/zmin-cli-schema/src/lib.rs'
      scope_start='    Instaweb {'
      scope_end='    Remote {'
      ;;
    schema.managed-hooks)
      scope_file_rel='crates/zmin-cli-schema/src/lib.rs'
      scope_start='pub enum ManagedHooksCommand {'
      scope_end='pub enum RefsCommand {'
      ;;
    schema.hook-run)
      scope_file_rel='crates/zmin-cli-schema/src/lib.rs'
      scope_start='pub enum HookCommand {'
      scope_end='pub enum ManagedHooksCommand {'
      ;;
    schema.hooks-add)
      scope_file_rel='crates/zmin-cli-schema/src/lib.rs'
      scope_start='pub enum ManagedHooksCommand {'
      scope_end='    List,'
      ;;
    schema.hooks-run)
      scope_file_rel='crates/zmin-cli-schema/src/lib.rs'
      scope_start='        #[arg(long = "staged", action = ArgAction::SetTrue)]'
      scope_end='    Remove {'
      ;;
    runtime.credential-cache)
      scope_file_rel='crates/zmin-cli/src/cli/commands/credential_impl.rs'
      scope_start='pub(crate) fn credential_cache('
      scope_end='fn credential_cache_socket_path('
      ;;
    runtime.http-version)
      scope_file_rel='crates/zmin-cli/src/cli/commands/transport_impl.rs'
      scope_start='fn remote_http_helper_version_arg_for_url('
      scope_end='fn should_force_http1_for_auto('
      ;;
    runtime.lfs-command)
      scope_file_rel='crates/zmin-cli/src/cli/commands/lfs_impl.rs'
      scope_start='pub(crate) fn lfs_command(args: Vec<String>) -> Result<()> {'
      scope_end='fn lfs_usage() -> String {'
      ;;
    contract.current-git-repo)
      scope_file_rel='tools/git-upstream-compat-contract.tsv'
      scope_start=$'current_git_non_extension_repo\t'
      scope_end="$scope_start"
      scope_same_line=1
      ;;
    *) return 1 ;;
  esac
  scope_file="$repo_root/$scope_file_rel"
}

validate_scope_binding() {
  local extension_id="$1"
  local scope_id="$2"
  local anchor="$3"
  local scope_result
  scope_definition "$scope_id" || {
    echo "unknown extension scope: $extension_id: $scope_id" >&2
    return 1
  }
  [[ -f "$scope_file" ]] || {
    echo "missing extension scope file: $extension_id: $scope_file_rel" >&2
    return 1
  }
  if [[ "$scope_same_line" == "1" ]]; then
    awk -v marker="$scope_start" -v anchor="$anchor" '
      index($0, marker) { marker_count += 1; if (index($0, anchor)) anchor_count += 1 }
      END { exit !(marker_count == 1 && anchor_count == 1) }
    ' "$scope_file" || {
      echo "extension scope boundary/anchor failure: $extension_id ($scope_id)" >&2
      return 1
    }
    return 0
  fi
  scope_result="$(awk -v start="$scope_start" -v end="$scope_end" -v anchor="$anchor" '
    index($0, start) { starts += 1; if (in_scope) nested_start = 1; in_scope = 1; start_line = NR }
    in_scope && index($0, end) { ends += 1; end_line = NR; in_scope = 0 }
    in_scope && index($0, anchor) { anchors += 1 }
    END {
      if (starts == 1 && ends == 1 && !in_scope && !nested_start && end_line > start_line && anchors == 1)
        print "ok"
    }
  ' "$scope_file")"
  [[ "$scope_result" == "ok" ]] || {
    echo "extension scope boundary/anchor failure: $extension_id ($scope_id)" >&2
    return 1
  }
}

validate_extension_contract() {
  [[ -f "$extension_contract_file" ]] || {
    echo "missing Zmin extension contract: $extension_contract_file" >&2
    return 2
  }
  local header
  header="$(head -n 1 "$extension_contract_file")"
  [[ "$header" == $'row_type\tid\tkind\tparent\tsurface\tevidence\tstatus\tanchor\tscope' ]] || {
    echo "invalid Zmin extension contract header" >&2
    return 1
  }

  local malformed duplicate primary_count relationship_count bad_row
  malformed="$(awk -F '\t' 'NR > 1 && (NF != 9 || ($1 != "primary" && $1 != "relationship") || $2 == "" || $3 == "" || $4 == "" || $5 == "" || $6 == "" || $7 == "" || $8 == "" || $9 == "") { print NR; exit }' "$extension_contract_file")"
  [[ -z "$malformed" ]] || { echo "malformed Zmin extension row: $malformed" >&2; return 1; }
  duplicate="$(awk -F '\t' 'NR > 1 { count[$2] += 1 } END { for (id in count) if (count[id] != 1) { print id; exit } }' "$extension_contract_file")"
  [[ -z "$duplicate" ]] || { echo "duplicate Zmin extension row: $duplicate" >&2; return 1; }
  local required_id
  for required_id in "${expected_extension_primary_ids[@]}" "${expected_extension_relationship_ids[@]}"; do
    if [[ "$(awk -F '\t' -v id="$required_id" '$2 == id { count += 1 } END { print count + 0 }' "$extension_contract_file")" == "0" ]]; then
      echo "Zmin extension required row missing: $required_id" >&2
      return 1
    fi
  done
  primary_count="$(awk -F '\t' 'NR > 1 && $1 == "primary" { count += 1 } END { print count + 0 }' "$extension_contract_file")"
  relationship_count="$(awk -F '\t' 'NR > 1 && $1 == "relationship" { count += 1 } END { print count + 0 }' "$extension_contract_file")"
  [[ "$primary_count" == "40" ]] || { echo "Zmin primary extension count drift: $primary_count/40" >&2; return 1; }
  [[ "$relationship_count" == "7" ]] || { echo "Zmin relationship count drift: $relationship_count/7" >&2; return 1; }

  local actual expected
  actual="$(awk -F '\t' 'NR > 1 && $1 == "primary" { print $2 }' "$extension_contract_file" | LC_ALL=C sort)"
  expected="$(printf '%s\n' "${expected_extension_primary_ids[@]}" | LC_ALL=C sort)"
  [[ "$actual" == "$expected" ]] || { echo "Zmin primary extension IDs drift" >&2; return 1; }
  actual="$(awk -F '\t' 'NR > 1 && $1 == "relationship" { print $2 }' "$extension_contract_file" | LC_ALL=C sort)"
  expected="$(printf '%s\n' "${expected_extension_relationship_ids[@]}" | LC_ALL=C sort)"
  [[ "$actual" == "$expected" ]] || { echo "Zmin relationship IDs drift" >&2; return 1; }

  local index extension_id extension_anchor actual_anchor expected_scope actual_scope
  for index in "${!expected_extension_primary_ids[@]}"; do
    extension_id="${expected_extension_primary_ids[$index]}"
    extension_anchor="${expected_extension_primary_anchors[$index]}"
    actual_anchor="$(awk -F '\t' -v id="$extension_id" '$2 == id { print $8; exit }' "$extension_contract_file")"
    [[ "$actual_anchor" == "$extension_anchor" ]] || {
      echo "extension evidence anchor drift: $extension_id" >&2
      return 1
    }
    expected_scope="$(extension_expected_scope "$extension_id")" || {
      echo "unknown expected extension scope: $extension_id" >&2
      return 1
    }
    actual_scope="$(awk -F '\t' -v id="$extension_id" '$2 == id { print $9; exit }' "$extension_contract_file")"
    [[ "$actual_scope" == "$expected_scope" ]] || {
      echo "extension scope drift: $extension_id" >&2
      return 1
    }
  done
  for index in "${!expected_extension_relationship_ids[@]}"; do
    extension_id="${expected_extension_relationship_ids[$index]}"
    extension_anchor="${expected_extension_relationship_anchors[$index]}"
    actual_anchor="$(awk -F '\t' -v id="$extension_id" '$2 == id { print $8; exit }' "$extension_contract_file")"
    [[ "$actual_anchor" == "$extension_anchor" ]] || {
      echo "extension evidence anchor drift: $extension_id" >&2
      return 1
    }
    expected_scope="$(extension_expected_scope "$extension_id")" || {
      echo "unknown expected extension scope: $extension_id" >&2
      return 1
    }
    actual_scope="$(awk -F '\t' -v id="$extension_id" '$2 == id { print $9; exit }' "$extension_contract_file")"
    [[ "$actual_scope" == "$expected_scope" ]] || {
      echo "extension scope drift: $extension_id" >&2
      return 1
    }
  done

  bad_row="$(awk -F '\t' '
    NR == 1 { next }
    $1 == "primary" {
      if ($3 != "command" && $3 != "subcommand" && $3 != "option" && $3 != "environment") { print NR; exit }
      if ($3 == "command" && $4 != "-") { print NR; exit }
      if ($3 == "subcommand" && $4 != "hooks") { print NR; exit }
      if ($3 == "environment" && $4 != "-") { print NR; exit }
      if ($3 == "option" && $4 == "") { print NR; exit }
      if ($2 ~ /^option\./ && $4 != "clone" && $4 != "cat-file" &&
          $4 != "imap-send" && $4 != "credential-cache" && $4 != "instaweb") { print NR; exit }
      if ($2 ~ /^hook-option\.hook-run\./ && $4 != "hook") { print NR; exit }
      if ($2 ~ /^hook-option\.hooks-(add|run)\./ && $4 != "hooks") { print NR; exit }
      if ($2 ~ /^command\./) {
        split($2, parts, "[.]")
        if ($5 != parts[2]) { print NR; exit }
      }
      if ($2 ~ /^subcommand\.hooks\./) {
        split($2, parts, "[.]")
        if ($5 != parts[3]) { print NR; exit }
      }
      if ($2 ~ /^(option|hook-option)\./) {
        part_count = split($2, parts, "[.]")
        expected_surface = "--" parts[part_count]
        if ($2 == "option.imap-send.short-folder") expected_surface = "-f"
        if ($5 != expected_surface) { print NR; exit }
      }
      if ($7 != "stable") { print NR; exit }
      if ($5 == "backfill" || $5 == "diff-pairs" || $5 == "format-rev" ||
          $5 == "history" || $5 == "last-modified" || $5 == "repo" || $5 == "url-parse") { print NR; exit }
    }
    $1 == "relationship" {
      if ($3 != "relationship" || ($4 != "hooks" && $4 != "repo") || $7 != "stable") { print NR; exit }
      if ($2 ~ /^relationship\.hooks\./ && $4 != "hooks") { print NR; exit }
      if ($2 ~ /^relationship\.repo\./ && $4 != "repo") { print NR; exit }
    }
  ' "$extension_contract_file")"
  [[ -z "$bad_row" ]] || { echo "invalid Zmin extension kind/parent/status/surface: $bad_row" >&2; return 1; }
  grep -q $'^primary\tcommand.hooks\t' "$extension_contract_file" || {
    echo "hooks relationship parent is not a primary extension" >&2
    return 1
  }
  [[ "$(contract_value current_git_non_extension_repo | cut -d'|' -f1)" == "repo" ]] || {
    echo "repo relationship parent is not pinned as current Git" >&2
    return 1
  }

  local row_type row_id kind parent surface evidence status anchor scope evidence_path canonical_evidence_path expected_scope
  while IFS=$'\t' read -r row_type row_id kind parent surface evidence status anchor scope; do
    [[ "$row_type" == "row_type" ]] && continue
    IFS=';' read -r -a evidence_paths <<< "$evidence"
    canonical_evidence_path="${evidence_paths[0]}"
    expected_scope="$(extension_expected_scope "$row_id")" || {
      echo "unknown expected extension scope: $row_id" >&2
      return 1
    }
    [[ "$scope" == "$expected_scope" ]] || {
      echo "extension scope drift: $row_id" >&2
      return 1
    }
    scope_definition "$scope" || {
      echo "unknown extension scope: $row_id: $scope" >&2
      return 1
    }
    [[ "$canonical_evidence_path" == "$scope_file_rel" ]] || {
      echo "extension scope evidence file drift: $row_id" >&2
      return 1
    }
    [[ -n "$canonical_evidence_path" ]] || {
      echo "missing canonical Zmin extension evidence path for $row_id" >&2
      return 1
    }
    for evidence_path in "${evidence_paths[@]}"; do
      [[ -f "$repo_root/$evidence_path" ]] || {
        echo "missing Zmin extension evidence path for $row_id: $evidence_path" >&2
        return 1
      }
    done
    validate_scope_binding "$row_id" "$scope" "$anchor" || {
      echo "evidence anchor missing for $row_id: $canonical_evidence_path: $anchor" >&2
      return 1
    }
  done < "$extension_contract_file"
}

validate_current_git_source_evidence() {
  local source_dir="$cache_root/git-$(contract_value upstream_git_tag)"
  local index current_key value command_name documentation_path test_paths status test_path
  [[ -d "$source_dir" ]] || { echo "missing pinned upstream source for current-command evidence" >&2; return 2; }
  for index in "${!current_git_non_extension_keys[@]}"; do
    current_key="${current_git_non_extension_keys[$index]}"
    value="$(contract_value "$current_key")"
    IFS='|' read -r command_name documentation_path test_paths status <<< "$value"
    [[ "$status" == "current-git-v2.55.0" ]] || { echo "current-command status drift: $current_key" >&2; return 1; }
    [[ -f "$source_dir/$documentation_path" ]] || { echo "missing pinned documentation evidence: $documentation_path" >&2; return 1; }
    IFS=',' read -r -a test_paths_array <<< "$test_paths"
    for test_path in "${test_paths_array[@]}"; do
      [[ -f "$source_dir/$test_path" ]] || { echo "missing pinned test evidence: $test_path" >&2; return 1; }
    done
  done
}

verify_remote_identity() {
  local tag repo expected actual archive_url
  tag="$(contract_value upstream_git_tag)"
  repo="$(contract_value upstream_git_repo)"
  expected="$(contract_value upstream_git_commit)"
  archive_url="$(contract_value upstream_archive_url)"

  [[ "$archive_url" == *"/refs/tags/$tag.tar.gz" ]] || {
    echo "archive URL is not pinned to contract tag $tag" >&2
    return 1
  }
  actual="$(cd "${TMPDIR:-/tmp}" && git ls-remote "$repo" "refs/tags/$tag^{}" | awk 'NR == 1 { print $1 }')"
  [[ -n "$actual" ]] || { echo "missing remote peeled commit for tag $tag" >&2; return 1; }
  if [[ "$actual" != "$expected" ]]; then
    echo "upstream tag commit mismatch: expected $expected, got $actual" >&2
    return 1
  fi
}

verify_source_tree() (
  local perl_path output
  perl_path="${ZMIN_HTTP_PERL:-${ZMIN_TEST_PERL:-$(command -v perl 2>/dev/null || true)}}"
  output="$(
    ZMIN_UPSTREAM_GIT_CACHE="$cache_root" ZMIN_HTTP_PERL="$perl_path" \
      "$repo_root/tools/git-upstream-http-provenance.sh" validate-source
  )" || {
    echo "upstream source identity verification failed" >&2
    return 1
  }
  [[ "$output" == source_manifest_sha256$'\t'* ]] || {
    echo "upstream source identity verifier returned malformed output" >&2
    return 1
  }
)

verify_cached_archive_identity() {
  local tag archive expected actual
  tag="$(contract_value upstream_git_tag)"
  archive="$cache_root/$tag.tar.gz"
  expected="$(contract_value upstream_archive_sha256)"
  [[ -f "$archive" && -s "$archive" ]] || {
    echo "missing canonical cached archive: $archive" >&2
    return 2
  }
  actual="$(shasum -a 256 "$archive" | awk '{ print $1 }')"
  [[ "$actual" == "$expected" ]] || {
    echo "cached archive SHA-256 mismatch: expected $expected, got $actual" >&2
    return 1
  }
}

verify_optional_checkout_head() {
  local tag expected candidate git_dir git_file actual
  tag="$(contract_value upstream_git_tag)"
  expected="$(contract_value upstream_git_commit)"
  for candidate in "$cache_root/git-$tag" "$cache_root/checkout-$tag"; do
    [[ -e "$candidate/.git" ]] || continue
    if [[ -f "$candidate/.git" ]]; then
      git_file="$(sed -n 's/^gitdir: //p' "$candidate/.git")"
      [[ -n "$git_file" ]] || {
        echo "invalid optional checkout gitdir: $candidate" >&2
        return 1
      }
      if [[ "$git_file" = /* ]]; then
        git_dir="$git_file"
      else
        git_dir="$(cd "$candidate/$git_file" 2>/dev/null && pwd)" || {
          echo "invalid optional checkout gitdir: $candidate" >&2
          return 1
        }
      fi
    else
      git_dir="$candidate/.git"
    fi
    actual="$(cd /tmp && GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
      git --git-dir="$git_dir" rev-parse --verify HEAD^{commit} 2>/dev/null)" || {
      echo "optional checkout HEAD unavailable: $candidate" >&2
      return 1
    }
    [[ "$actual" == "$expected" ]] || {
      echo "optional checkout HEAD mismatch: expected $expected, got $actual" >&2
      return 1
    }
  done
}

run_contract_check() {
  verify_cached_archive_identity
  verify_optional_checkout_head
  verify_source_tree
  validate_current_git_source_evidence
}

print_contract_identity() {
  printf 'contract\t%s\tcommit=%s\tarchive_sha256=%s\tmanifest=%s\tdenominator=%s\n' \
    "$(contract_value upstream_git_tag)" \
    "$(contract_value upstream_git_commit)" \
    "$(contract_value upstream_archive_sha256)" \
    "$(contract_value authoritative_manifest)" \
    "$(contract_value authoritative_upstream_test_denominator)"
}

acquire_contract_cache_lock() {
  local timeout="${ZMIN_CONTRACT_CACHE_LOCK_TIMEOUT_SECONDS:-60}" started now
  [[ "$timeout" =~ ^[0-9]+$ ]] || {
    echo "invalid compatibility contract cache lock timeout" >&2
    return 1
  }
  [[ ! -L "$cache_root" ]] || {
    echo "compatibility contract cache root is a symlink: $cache_root" >&2
    return 1
  }
  mkdir -p "$cache_root"
  started="$(date +%s)"
  while :; do
    if [[ -L "$contract_cache_lock_dir" ||
      ( -e "$contract_cache_lock_dir" && ! -d "$contract_cache_lock_dir" ) ]]; then
      echo "compatibility contract cache lock path is invalid: $contract_cache_lock_dir" >&2
      return 1
    fi
    if mkdir "$contract_cache_lock_dir" 2>/dev/null; then
      contract_cache_lock_token="$$.$RANDOM.$(date +%s)"
      if ! printf '%s\n' "$contract_cache_lock_token" >"$contract_cache_lock_dir/owner"; then
        rm -f "$contract_cache_lock_dir/owner"
        rmdir "$contract_cache_lock_dir" 2>/dev/null || true
        echo "unable to initialize compatibility contract cache lock: $contract_cache_lock_dir" >&2
        return 1
      fi
      contract_cache_lock_held=1
      return 0
    fi
    now="$(date +%s)"
    if (( now - started >= timeout )); then
      echo "timed out waiting for compatibility contract cache lock: $contract_cache_lock_dir" >&2
      return 1
    fi
    sleep 1
  done
}

release_contract_cache_lock() {
  if [[ "$contract_cache_lock_held" == "1" &&
    -f "$contract_cache_lock_dir/owner" && ! -L "$contract_cache_lock_dir/owner" &&
    "$(cat "$contract_cache_lock_dir/owner")" == "$contract_cache_lock_token" ]]; then
    rm -f "$contract_cache_lock_dir/owner"
    rmdir "$contract_cache_lock_dir" 2>/dev/null || true
  fi
  contract_cache_lock_held=0
  contract_cache_lock_token=""
}

cleanup_contract_cache_prepare() {
  if [[ -n "${contract_cache_prepare_temp:-}" && -d "$contract_cache_prepare_temp" ]]; then
    chmod -R u+w "$contract_cache_prepare_temp" 2>/dev/null || true
    rm -rf "$contract_cache_prepare_temp"
  fi
  release_contract_cache_lock
}

validate_prepared_contract_cache() {
  local tag archive source_dir
  tag="$(contract_value upstream_git_tag)"
  archive="$cache_root/$tag.tar.gz"
  source_dir="$cache_root/git-$tag"
  [[ -f "$archive" && ! -L "$archive" && -s "$archive" &&
    -d "$source_dir" && ! -L "$source_dir" ]] || {
    echo "prepared compatibility contract cache is incomplete: $cache_root" >&2
    return 2
  }
  verify_cached_archive_identity
  verify_source_tree
  verify_optional_checkout_head
}

prepare_cache() (
  local tag archive_url archive_sha archive source_dir source_root actual_sha
  contract_cache_prepare_temp=""
  tag="$(contract_value upstream_git_tag)"
  archive_url="$(contract_value upstream_archive_url)"
  archive_sha="$(contract_value upstream_archive_sha256)"
  archive="$cache_root/$tag.tar.gz"
  source_dir="$cache_root/git-$tag"
  source_root="git-${tag#v}"

  trap cleanup_contract_cache_prepare EXIT
  acquire_contract_cache_lock

  if [[ -e "$archive" || -L "$archive" || -e "$source_dir" || -L "$source_dir" ]]; then
    [[ -f "$archive" && ! -L "$archive" && -s "$archive" &&
      -d "$source_dir" && ! -L "$source_dir" ]] || {
      echo "partial contract cache exists; refusing to replace it: $cache_root" >&2
      return 2
    }
    validate_prepared_contract_cache
    return 0
  fi

  contract_cache_prepare_temp="$(mktemp -d "$cache_root/.zmin-upstream-contract-gate.XXXXXX")"
  curl --fail --location --retry 3 --silent --show-error "$archive_url" \
    -o "$contract_cache_prepare_temp/archive.tar.gz"
  actual_sha="$(shasum -a 256 "$contract_cache_prepare_temp/archive.tar.gz" | awk '{ print $1 }')"
  [[ "$actual_sha" == "$archive_sha" ]] || {
    echo "downloaded archive SHA-256 mismatch: expected $archive_sha, got $actual_sha" >&2
    return 1
  }
  tar -xzf "$contract_cache_prepare_temp/archive.tar.gz" -C "$contract_cache_prepare_temp"
  [[ -d "$contract_cache_prepare_temp/$source_root/t" ]] || {
    echo "archive does not contain expected source tree $source_root/t" >&2
    return 1
  }
  printf '%s\n' "$archive_sha" >"$contract_cache_prepare_temp/$source_root/.zmin-pristine-source.sha256"
  chmod -R a-w "$contract_cache_prepare_temp/$source_root"

  [[ ! -e "$archive" && ! -L "$archive" ]] || {
    echo "compatibility contract archive publication collision" >&2
    validate_prepared_contract_cache
    return 1
  }
  mv "$contract_cache_prepare_temp/archive.tar.gz" "$archive"
  [[ ! -e "$contract_cache_prepare_temp/archive.tar.gz" ]] || {
    echo "compatibility contract archive publication collision" >&2
    validate_prepared_contract_cache
    return 1
  }
  # macOS requires the directory being renamed to be writable even when its
  # contents are already readonly. Restore that single rename permission only
  # for the atomic move, then enforce the verifier's readonly tree invariant.
  chmod u+w "$contract_cache_prepare_temp/$source_root"
  [[ ! -e "$source_dir" && ! -L "$source_dir" ]] || {
    echo "compatibility contract source publication collision" >&2
    validate_prepared_contract_cache
    return 1
  }
  mv "$contract_cache_prepare_temp/$source_root" "$source_dir"
  [[ ! -e "$contract_cache_prepare_temp/$source_root" ]] || {
    echo "compatibility contract source publication collision" >&2
    validate_prepared_contract_cache
    return 1
  }
  chmod -R a-w "$source_dir"
  validate_prepared_contract_cache
)

metadata_value() {
  local key="$1"
  awk -F '\t' -v key="$key" '$1 == key { print $2; exit }' "$metadata_file"
}

test_name_digest() {
  awk -F '\t' 'NR > 1 { print $2 }' "$1" | shasum -a 256 | awk '{ print $1 }'
}

validate_authoritative_manifest() {
  local manifest_file="$1"
  local temp_root="$2"
  local source_dir="$cache_root/git-$(contract_value upstream_git_tag)"
  local expected_names="$temp_root/expected-upstream-names"
  local actual_names="$temp_root/actual-upstream-names"
  local source_file test_name total_count excluded_count excluded_name bad_row
  [[ -d "$source_dir/t" ]] || { echo "missing pinned source test directory" >&2; return 2; }
  [[ "$(head -n 1 "$manifest_file")" == $'# mode\ttest\treason' ]] || {
    echo "invalid authoritative manifest header" >&2
    return 1
  }
  bad_row="$(awk -F '\t' '
    NR == 1 { next }
    NF != 3 || $1 != "all-nondeprecated" || $2 !~ /^t[0-9][0-9][0-9][0-9]-[^\/]+\.sh$/ ||
      $3 != "complete upstream top-level shell suite minus explicit whole-file deprecated excludes" {
      print NR
      exit
    }
  ' "$manifest_file")"
  [[ -z "$bad_row" ]] || { echo "invalid authoritative manifest row: $bad_row" >&2; return 1; }

  : >"$expected_names"
  total_count=0
  excluded_count=0
  excluded_name=""
  for source_file in "$source_dir"/t/t[0-9][0-9][0-9][0-9]-*.sh; do
    [[ -f "$source_file" ]] || continue
    test_name="${source_file##*/}"
    total_count=$((total_count + 1))
    if [[ "$test_name" == "t5323-pack-redundant.sh" ]]; then
      excluded_count=$((excluded_count + 1))
      excluded_name="$test_name"
    else
      printf '%s\n' "$test_name" >>"$expected_names"
    fi
  done
  [[ "$total_count" == "$EXPECTED_TOTAL" ]] || {
    echo "pinned top-level test count drift: $total_count/$EXPECTED_TOTAL" >&2
    return 1
  }
  [[ "$excluded_count" == "$EXPECTED_DEPRECATED" && "$excluded_name" == "t5323-pack-redundant.sh" ]] || {
    echo "deprecated whole-file exclusion drift" >&2
    return 1
  }
  LC_ALL=C sort -u "$expected_names" -o "$expected_names"
  awk -F '\t' 'NR > 1 { print $2 }' "$manifest_file" | LC_ALL=C sort -u >"$actual_names"
  if ! cmp -s "$expected_names" "$actual_names"; then
    echo "authoritative manifest does not equal pinned all-nondeprecated source set" >&2
    return 1
  fi
  if [[ "$(wc -l <"$actual_names" | tr -d ' ')" != "$EXPECTED_DENOMINATOR" ]]; then
    echo "authoritative manifest denominator drift" >&2
    return 1
  fi
  if grep -Fq 't5323-pack-redundant.sh' "$actual_names"; then
    echo "sole deprecated file appears in authoritative manifest" >&2
    return 1
  fi
}

validate_run() (
  local temp_root
  temp_root="$(mktemp -d "${TMPDIR:-/tmp}/zmin-contract-run-validation.XXXXXX")"
  trap 'rm -rf "$temp_root"' EXIT
  [[ -f "$metadata_file" ]] || { echo "missing run metadata: $metadata_file" >&2; return 2; }
  local key count value
  for key in upstream_git_tag upstream_git_commit upstream_archive_sha256 mode scope evidence_scope compatibility_claim manifest_file manifest_sha256 manifest_test_count summary_file summary_sha256 manifest_offset manifest_limit total passed failed; do
    count="$(awk -F '\t' -v key="$key" '$1 == key { count += 1 } END { print count + 0 }' "$metadata_file")"
    value="$(metadata_value "$key")"
    if [[ "$count" != "1" || -z "$value" ]]; then
      echo "run metadata key must occur exactly once with a value: $key" >&2
      return 1
    fi
  done

  local denominator tag commit archive_sha authoritative
  denominator="$(contract_value authoritative_upstream_test_denominator)"
  tag="$(contract_value upstream_git_tag)"
  commit="$(contract_value upstream_git_commit)"
  archive_sha="$(contract_value upstream_archive_sha256)"
  if [[ "$require_authoritative" == "1" || "$(metadata_value evidence_scope)" == "authoritative-upstream-suite" ]]; then
    run_contract_check >/dev/null
  fi

  local manifest_file summary_file actual_manifest_sha actual_summary_sha actual_manifest_count
  manifest_file="$(metadata_value manifest_file)"
  summary_file="$(metadata_value summary_file)"
  [[ -f "$manifest_file" ]] || { echo "missing manifest from run metadata: $manifest_file" >&2; return 1; }
  [[ -f "$summary_file" ]] || { echo "missing summary from run metadata: $summary_file" >&2; return 1; }
  actual_manifest_sha="$(test_name_digest "$manifest_file")"
  actual_summary_sha="$(shasum -a 256 "$summary_file" | awk '{ print $1 }')"
  actual_manifest_count="$(awk -F '\t' 'NR > 1 { count += 1 } END { print count + 0 }' "$manifest_file")"
  [[ "$(metadata_value manifest_sha256)" == "$actual_manifest_sha" ]] || { echo "manifest digest drift" >&2; return 1; }
  [[ "$(metadata_value summary_sha256)" == "$actual_summary_sha" ]] || { echo "summary digest drift" >&2; return 1; }
  [[ "$(metadata_value manifest_test_count)" == "$actual_manifest_count" ]] || { echo "manifest count drift" >&2; return 1; }

  local manifest_names="$temp_root/manifest-names" summary_names="$temp_root/summary-names"
  awk -F '\t' 'NR > 1 && NF >= 2 { print $2 }' "$manifest_file" >"$manifest_names"
  if ! awk -F '\t' '
    NR == 1 { next }
    NF != 5 || ($3 != "pass" && $3 != "fail") { bad = 1; next }
    { print $2 }
    END { exit bad }
  ' "$summary_file" >"$summary_names"; then
    echo "invalid suite summary shape or status" >&2
    return 1
  fi
  if [[ -n "$(LC_ALL=C sort "$manifest_names" | uniq -d)" ]]; then
    echo "duplicate test in run manifest" >&2
    return 1
  fi
  cmp -s "$manifest_names" "$summary_names" || {
    echo "run summary test set differs from run manifest" >&2
    return 1
  }

  if [[ "$require_authoritative" == "1" || "$(metadata_value evidence_scope)" == "authoritative-upstream-suite" ]]; then
    validate_authoritative_manifest "$manifest_file" "$temp_root"
  fi
  authoritative=0
  if [[ "$(metadata_value upstream_git_tag)" == "$tag" &&
    "$(metadata_value upstream_git_commit)" == "$commit" &&
    "$(metadata_value upstream_archive_sha256)" == "$archive_sha" &&
    "$(metadata_value mode)" == "all-nondeprecated" &&
    "$(metadata_value scope)" == "current-contract" &&
    "$(metadata_value evidence_scope)" == "authoritative-upstream-suite" &&
    "$(metadata_value compatibility_claim)" == "unverified" &&
    "$(metadata_value manifest_offset)" == "0" &&
    "$(metadata_value manifest_limit)" == "0" &&
    "$(metadata_value total)" == "$denominator" &&
    "$(metadata_value passed)" == "$denominator" &&
    "$(metadata_value failed)" == "0" &&
    "$(metadata_value manifest_test_count)" == "$denominator" &&
    "$(metadata_value total)" == "$actual_manifest_count" &&
    "$(metadata_value failed)" == "$(awk -F '\t' 'NR > 1 && $3 == "fail" { count += 1 } END { print count + 0 }' "$summary_file")" ]]; then
    authoritative=1
  fi

  if [[ "$authoritative" == "1" ]]; then
    printf 'run_scope=authoritative-upstream-suite\n'
  else
    printf 'run_scope=exploratory-or-incomplete\n'
    if [[ "$require_authoritative" == "1" ]]; then
      echo "run metadata is not authoritative full upstream evidence" >&2
      return 1
    fi
  fi
  printf 'compatibility_claim=unverified\n'
)

case "$mode" in
  check)
    validate_contract_shape
    verify_remote_identity
    run_contract_check
    print_contract_identity
    printf 'contract_gate=pass\n'
    printf 'zmin_extension_contract=pass\tprimary=40\trelationships=7\n'
    printf 'evidence_scope=scope-contract\n'
    printf 'compatibility_claim=unverified\n'
    ;;
  prepare-and-check)
    validate_contract_shape
    verify_remote_identity
    prepare_cache
    run_contract_check
    print_contract_identity
    printf 'contract_gate=pass\n'
    printf 'zmin_extension_contract=pass\tprimary=40\trelationships=7\n'
    printf 'evidence_scope=scope-contract\n'
    printf 'compatibility_claim=unverified\n'
    ;;
  validate-run)
    validate_contract_shape
    validate_run
    ;;
esac
