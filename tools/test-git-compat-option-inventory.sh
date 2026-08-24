#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
inventory="$repo_root/tools/git-compat-option-inventory.sh"
upstream_cache="${ZMIN_UPSTREAM_GIT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zmin/git-upstream}"
pinned_source="$upstream_cache/git-v2.55.0"
archive_sha="$(awk -F '\t' '$1 == "upstream_archive_sha256" { print $2 }' "$repo_root/tools/git-upstream-compat-contract.tsv")"
[[ -n "$archive_sha" ]] || { printf 'FAIL: upstream archive identity is missing\n' >&2; exit 1; }
temp_root="$(mktemp -d "${TMPDIR:-/tmp}/zmin-option-inventory.XXXXXX")"
trap 'rm -rf "$temp_root"' EXIT

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

assert_contains() {
  local file="$1"
  local text="$2"
  grep -Fqx "$text" "$file" || fail "missing output row: $text"
}

expect_failure() {
  local label="$1"
  local diagnostic="$2"
  shift 2
  local output rc
  set +e
  output="$("$@" 2>&1)"
  rc=$?
  set -e
  [[ "$rc" -ne 0 ]] || fail "$label unexpectedly succeeded"
  [[ "$output" == *"$diagnostic"* ]] || {
    printf '%s\n' "$output" >&2
    fail "$label missed diagnostic: $diagnostic"
  }
  printf 'expected-failure\t%s\n' "$label"
}

sha256_tool="$(command -v sha256sum || true)"
sha256_mode='sha256sum'
if [[ -z "$sha256_tool" ]]; then
  sha256_tool="$(command -v shasum || true)"
  sha256_mode='shasum'
fi
[[ -n "$sha256_tool" ]] || fail 'no portable SHA-256 helper is available'

sha256_file() {
  if [[ "$sha256_mode" == 'sha256sum' ]]; then
    "$sha256_tool" "$1"
  else
    "$sha256_tool" -a 256 "$1"
  fi
}

make_fixture() {
  local root="$1"
  mkdir -p "$root/Documentation/sub"
  printf '%s\n' "$archive_sha" > "$root/.zmin-pristine-source.sha256"
  printf 'git-log\tcommon\n' > "$root/command-list.txt"
  cat > "$root/Documentation/git-log.adoc" <<'EOF'
git-log(1)
==========

OPTIONS
-------

`--follow`::
    Follow a file.
--[no-]decorate::
    Decorate output.
include::sub/options.adoc[]
include::{build_dir}/mergetools-diff.adoc[]
EOF
  cat > "$root/Documentation/sub/options.adoc" <<'EOF'
OPTIONS
-------

--nested=<value>::
    A nested option include.
EOF
}

run_fixture() {
  local root="$1"
  shift
  env \
    ZMIN_GIT_BASELINE=v2.55.0 \
    ZMIN_GIT_DOC_CACHE="$root" \
    ZMIN_GIT_COMMAND_LIST="$root/command-list.txt" \
    ZMIN_GIT_SOURCE_ARCHIVE_SHA256="$archive_sha" \
    "$@" \
    "$inventory"
}

snapshot_source() {
  local root="$1"
  local output="$2"
  (
    cd "$root"
    [[ -n "$(find . -type f -print -quit)" ]] || fail "source snapshot is empty: $root"
    if [[ "$sha256_mode" == 'sha256sum' ]]; then
      find . -type f -print0 | xargs -0 "$sha256_tool"
    else
      find . -type f -print0 | xargs -0 "$sha256_tool" -a 256
    fi |
      awk '{ digest = $1; sub(/^[^[:space:]]+[[:space:]]+/, ""); sub(/^\.\//, ""); print digest "\t" $0 }' |
      LC_ALL=C sort
  ) > "$output"
}

[[ -d "$pinned_source/Documentation" ]] || fail "pinned source is unavailable: $pinned_source"

sha_probe="$temp_root/sha-probe"
printf 'test' > "$sha_probe"
[[ "$(sha256_file "$sha_probe" | awk '{ print $1 }')" == \
  9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08 ]] || {
  fail 'portable SHA-256 helper returned an unexpected digest'
}

before="$temp_root/source-before.tsv"
after="$temp_root/source-after.tsv"
snapshot_source "$pinned_source" "$before"

sentinel_bin="$temp_root/sentinel-bin"
mkdir -p "$sentinel_bin"
cat > "$sentinel_bin/curl" <<'EOF'
#!/usr/bin/env bash
printf 'curl-called\n' >> "${ZMIN_OPTION_CURL_SENTINEL:?}"
exit 99
EOF
chmod +x "$sentinel_bin/curl"
sentinel_log="$temp_root/curl.log"
positive="$temp_root/pinned.tsv"
(
  cd "$temp_root"
  env \
    PATH="$sentinel_bin:$PATH" \
    ZMIN_OPTION_CURL_SENTINEL="$sentinel_log" \
    ZMIN_GIT_BASELINE=v2.55.0 \
    ZMIN_GIT_DOC_CACHE="$pinned_source" \
    ZMIN_GIT_COMMAND_LIST="$pinned_source/command-list.txt" \
    ZMIN_GIT_SOURCE_ARCHIVE_SHA256="$archive_sha" \
    "$inventory" > "$positive"
)
[[ ! -e "$sentinel_log" ]] || fail 'network sentinel was invoked'
assert_contains "$positive" $'log\t--follow\tgit-log.adoc'
assert_contains "$positive" $'reset\t--quiet\tgit-reset.adoc'
for row in \
  $'diff\t-0\tgit-diff.adoc' \
  $'diff\t-1\tgit-diff.adoc' \
  $'diff\t-2\tgit-diff.adoc' \
  $'diff\t-3\tgit-diff.adoc' \
  $'fetch\t-4\tgit-fetch.adoc' \
  $'fetch\t-6\tgit-fetch.adoc' \
  $'cat-file\t-e\tgit-cat-file.adoc' \
  $'clone\t--local\tgit-clone.adoc' \
  $'clone\t--no-hardlinks\tgit-clone.adoc' \
  $'imap-send\t-f\tgit-imap-send.adoc' \
  $'imap-send\t--folder\tgit-imap-send.adoc' \
  $'imap-send\t--list\tgit-imap-send.adoc' \
  $'credential-cache\t--timeout\tgit-credential-cache.adoc' \
  $'credential-cache\t--socket\tgit-credential-cache.adoc' \
  $'instaweb\t--httpd\tgit-instaweb.adoc' \
  $'instaweb\t--port\tgit-instaweb.adoc' \
  $'instaweb\t--browser\tgit-instaweb.adoc' \
  $'cat-file\t--no-filter\tgit-cat-file.adoc' \
  $'log\t--no-decorate\tgit-log.adoc'; do
  assert_contains "$positive" "$row"
done
if grep -F '.txt' "$positive" >/dev/null; then
  fail 'legacy .txt documentation appeared in output'
fi
snapshot_source "$pinned_source" "$after"
cmp -s "$before" "$after" || fail 'pinned source changed during inventory'
printf 'positive\tpinned-v2.55-adoc\n'

fixture="$temp_root/git-v2.55.0"
make_fixture "$fixture"
fixture_output="$temp_root/fixture.tsv"
run_fixture "$fixture" env PATH="$PATH" > "$fixture_output"
assert_contains "$fixture_output" $'log\t--follow\tgit-log.adoc'
assert_contains "$fixture_output" $'log\t--decorate\tgit-log.adoc'
assert_contains "$fixture_output" $'log\t--no-decorate\tgit-log.adoc'
assert_contains "$fixture_output" $'log\t--nested\tgit-log.adoc'
printf 'positive\tnested-include-and-negation\n'

expect_failure unset-env 'ZMIN_GIT_BASELINE is required' \
  env -i PATH="$PATH" "$inventory"
expect_failure partial-env 'ZMIN_GIT_DOC_CACHE is required' \
  env -i PATH="$PATH" ZMIN_GIT_BASELINE=v2.55.0 "$inventory"

old_root="$temp_root/git-v2.47.1"
make_fixture "$old_root"
mv "$old_root/Documentation/git-log.adoc" "$old_root/Documentation/git-log.txt"
expect_failure old-tag 'unsupported Git source tag: v2.47.1' \
  env ZMIN_GIT_BASELINE=v2.47.1 ZMIN_GIT_DOC_CACHE="$old_root" \
    ZMIN_GIT_COMMAND_LIST="$old_root/command-list.txt" \
    ZMIN_GIT_SOURCE_ARCHIVE_SHA256="$archive_sha" "$inventory"
txt_root="$temp_root/txtcase/git-v2.55.0"
make_fixture "$txt_root"
mv "$txt_root/Documentation/git-log.adoc" "$txt_root/Documentation/git-log.txt"
expect_failure txt-only 'missing Git command documentation: git-log.adoc' \
  env ZMIN_GIT_BASELINE=v2.55.0 ZMIN_GIT_DOC_CACHE="$txt_root" \
    ZMIN_GIT_COMMAND_LIST="$txt_root/command-list.txt" \
    ZMIN_GIT_SOURCE_ARCHIVE_SHA256="$archive_sha" "$inventory"

no_doc_root="$temp_root/nodoccase/git-v2.55.0"
mkdir -p "$no_doc_root"
printf '%s\n' "$archive_sha" > "$no_doc_root/.zmin-pristine-source.sha256"
printf 'git-log\tcommon\n' > "$no_doc_root/command-list.txt"
expect_failure missing-documentation 'Git source Documentation directory is missing' \
  env ZMIN_GIT_BASELINE=v2.55.0 ZMIN_GIT_DOC_CACHE="$no_doc_root" \
    ZMIN_GIT_COMMAND_LIST="$no_doc_root/command-list.txt" \
    ZMIN_GIT_SOURCE_ARCHIVE_SHA256="$archive_sha" "$inventory"

mismatch_root="$temp_root/source-mismatch"
cp -R "$fixture" "$mismatch_root"
expect_failure source-basename 'Git source root basename does not match v2.55.0' \
  env ZMIN_GIT_BASELINE=v2.55.0 ZMIN_GIT_DOC_CACHE="$mismatch_root" \
    ZMIN_GIT_COMMAND_LIST="$mismatch_root/command-list.txt" \
    ZMIN_GIT_SOURCE_ARCHIVE_SHA256="$archive_sha" "$inventory"

printf '%s\n' 0000000000000000000000000000000000000000000000000000000000000000 > "$fixture/.zmin-pristine-source.sha256"
expect_failure wrong-marker-value 'Git source identity mismatch' run_fixture "$fixture" env PATH="$PATH"
printf '%s\n' "$archive_sha" > "$fixture/.zmin-pristine-source.sha256"

missing_marker_root="$temp_root/missingmarkercase/git-v2.55.0"
mkdir -p "$(dirname "$missing_marker_root")"
cp -R "$fixture" "$missing_marker_root"
rm -f "$missing_marker_root/.zmin-pristine-source.sha256"
expect_failure missing-marker 'Git source identity marker is missing' \
  env ZMIN_GIT_BASELINE=v2.55.0 ZMIN_GIT_DOC_CACHE="$missing_marker_root" \
    ZMIN_GIT_COMMAND_LIST="$missing_marker_root/command-list.txt" \
    ZMIN_GIT_SOURCE_ARCHIVE_SHA256="$archive_sha" "$inventory"

outside_list="$temp_root/arbitrary-command-list.txt"
printf 'git-log\tcommon\n' > "$outside_list"
expect_failure command-list-outside 'Git command list is not the validated source command-list.txt' \
  env ZMIN_GIT_BASELINE=v2.55.0 ZMIN_GIT_DOC_CACHE="$fixture" \
    ZMIN_GIT_COMMAND_LIST="$outside_list" ZMIN_GIT_SOURCE_ARCHIVE_SHA256="$archive_sha" "$inventory"

printf 'git-missing\tcommon\n' > "$fixture/command-list.txt"
expect_failure missing-command-doc 'missing Git command documentation: git-missing.adoc' \
  run_fixture "$fixture" env PATH="$PATH"
printf 'git-log\tcommon\n' > "$fixture/command-list.txt"

drop_last_line() {
  local file="$1"
  local temporary="$file.tmp"
  sed '$d' "$file" > "$temporary"
  mv "$temporary" "$file"
}

printf 'include::missing.adoc[]\n' >> "$fixture/Documentation/git-log.adoc"
expect_failure missing-include 'missing AsciiDoc include: missing.adoc' \
  run_fixture "$fixture" env PATH="$PATH"
drop_last_line "$fixture/Documentation/git-log.adoc"

printf 'include::../escape.adoc[]\n' >> "$fixture/Documentation/git-log.adoc"
printf 'outside\n' > "$fixture/escape.adoc"
expect_failure include-escape 'AsciiDoc include escapes Documentation' \
  run_fixture "$fixture" env PATH="$PATH"
drop_last_line "$fixture/Documentation/git-log.adoc"

cycle_root="$temp_root/cyclecase/git-v2.55.0"
mkdir -p "$cycle_root/Documentation"
printf '%s\n' "$archive_sha" > "$cycle_root/.zmin-pristine-source.sha256"
printf 'git-a\tcommon\n' > "$cycle_root/command-list.txt"
printf 'include::b.adoc[]\n' > "$cycle_root/Documentation/git-a.adoc"
printf 'include::git-a.adoc[]\n' > "$cycle_root/Documentation/b.adoc"
expect_failure include-cycle 'AsciiDoc include cycle' \
  env ZMIN_GIT_BASELINE=v2.55.0 ZMIN_GIT_DOC_CACHE="$cycle_root" \
    ZMIN_GIT_COMMAND_LIST="$cycle_root/command-list.txt" \
    ZMIN_GIT_SOURCE_ARCHIVE_SHA256="$archive_sha" "$inventory"

printf 'option-inventory-tests=pass\n'
