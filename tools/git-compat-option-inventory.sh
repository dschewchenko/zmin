#!/usr/bin/env bash
set -euo pipefail

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

[[ -n "${ZMIN_GIT_BASELINE:-}" ]] || die 'ZMIN_GIT_BASELINE is required'
[[ -n "${ZMIN_GIT_DOC_CACHE:-}" ]] || die 'ZMIN_GIT_DOC_CACHE is required'
[[ -n "${ZMIN_GIT_COMMAND_LIST:-}" ]] || die 'ZMIN_GIT_COMMAND_LIST is required'
[[ -n "${ZMIN_GIT_SOURCE_ARCHIVE_SHA256:-}" ]] || die 'ZMIN_GIT_SOURCE_ARCHIVE_SHA256 is required'

baseline="$ZMIN_GIT_BASELINE"
[[ "$baseline" == 'v2.55.0' ]] || die "unsupported Git source tag: $baseline"

canonical_dir() {
  local directory="$1"
  cd "$directory" 2>/dev/null && pwd -P
}

canonical_file() {
  local file_path="$1"
  local parent
  parent="$(canonical_dir "$(dirname "$file_path")")" || return 1
  printf '%s/%s\n' "$parent" "$(basename "$file_path")"
}

source_root="$(canonical_dir "$ZMIN_GIT_DOC_CACHE")" || die "source root is not a directory: $ZMIN_GIT_DOC_CACHE"
[[ "$(basename "$source_root")" == "git-$baseline" ]] || {
  die "Git source root basename does not match $baseline: $source_root"
}

doc_root="$source_root/Documentation"
[[ -d "$doc_root" ]] || die "Git source Documentation directory is missing: $doc_root"

command_list="$(canonical_file "$ZMIN_GIT_COMMAND_LIST")" || {
  die "Git command list is not readable: $ZMIN_GIT_COMMAND_LIST"
}
[[ -f "$command_list" && ! -L "$ZMIN_GIT_COMMAND_LIST" ]] || {
  die "Git command list is not a regular file: $ZMIN_GIT_COMMAND_LIST"
}
[[ "$command_list" == "$source_root/command-list.txt" ]] || {
  die "Git command list is not the validated source command-list.txt: $ZMIN_GIT_COMMAND_LIST"
}

marker="$source_root/.zmin-pristine-source.sha256"
[[ -f "$marker" && ! -L "$marker" ]] || die "Git source identity marker is missing: $marker"
expected_archive_sha="$ZMIN_GIT_SOURCE_ARCHIVE_SHA256"
[[ "$expected_archive_sha" =~ ^[0-9a-f]{64}$ ]] || {
  die 'ZMIN_GIT_SOURCE_ARCHIVE_SHA256 must be a lowercase SHA-256'
}
marker_value="$(awk 'NR == 1 { value = $0 } NR > 1 { extra = 1 } END { if (extra || value == "") exit 1; print value }' "$marker")" || {
  die "Git source identity marker is malformed: $marker"
}
[[ "$marker_value" == "$expected_archive_sha" ]] || {
  die "Git source identity mismatch: expected $expected_archive_sha, got $marker_value"
}

commands="$(awk '$1 ~ /^git-/ { command = $1; sub(/^git-/, "", command); print command }' "$command_list" | LC_ALL=C sort -u)"
[[ -n "$commands" ]] || die 'validated Git command-list.txt contains no commands'

stack_depth=0
stack_contains() {
  local path="$1"
  local index
  for ((index = 0; index < stack_depth; index++)); do
    [[ "${include_stack[$index]}" == "$path" ]] && return 0
  done
  return 1
}

is_known_generated_build_include() {
  case "$1" in
    '{build_dir}/cmds-ancillaryinterrogators.adoc'|'{build_dir}/cmds-ancillarymanipulators.adoc'|'{build_dir}/cmds-developerinterfaces.adoc'|'{build_dir}/cmds-foreignscminterface.adoc'|'{build_dir}/cmds-guide.adoc'|'{build_dir}/cmds-mainporcelain.adoc'|'{build_dir}/cmds-plumbinginterrogators.adoc'|'{build_dir}/cmds-plumbingmanipulators.adoc'|'{build_dir}/cmds-purehelpers.adoc'|'{build_dir}/cmds-synchelpers.adoc'|'{build_dir}/cmds-synchingrepositories.adoc'|'{build_dir}/cmds-userinterfaces.adoc'|'{build_dir}/mergetools-diff.adoc'|'{build_dir}/mergetools-merge.adoc')
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

parse_doc() {
  local file_path="$1"
  local line include_path candidate resolved include_dir

  stack_contains "$file_path" && die "AsciiDoc include cycle: $file_path"
  include_stack[$stack_depth]="$file_path"
  stack_depth=$((stack_depth + 1))

  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" =~ ^[[:space:]]*include::([^[]+)\[\][[:space:]]*$ ]]; then
      include_path="${BASH_REMATCH[1]}"
      # Git v2.55's source checkout has no generated build directory. These
      # exact fragments contain command/tool indexes, not OPTIONS entries;
      # all ordinary relative includes remain mandatory and are parsed.
      if is_known_generated_build_include "$include_path"; then
        continue
      fi
      [[ "$include_path" != /* ]] || die "absolute AsciiDoc include is forbidden: $include_path"
      include_dir="$(canonical_dir "$(dirname "$file_path")")" || die "include directory is unavailable: $file_path"
      candidate="$include_dir/$include_path"
      [[ -f "$candidate" && ! -L "$candidate" ]] || {
        die "missing AsciiDoc include: $include_path from $file_path"
      }
      resolved="$(canonical_file "$candidate")" || die "cannot resolve AsciiDoc include: $include_path"
      case "$resolved" in
        "$doc_root"/*) ;;
        *) die "AsciiDoc include escapes Documentation: $include_path from $file_path" ;;
      esac
      parse_doc "$resolved"
    else
      printf '%s\n' "$line"
    fi
  done < "$file_path"

  stack_depth=$((stack_depth - 1))
  unset 'include_stack[stack_depth]'
}

printf 'command\toption\tdoc\n'
while IFS= read -r command; do
  [[ -n "$command" ]] || continue
  [[ "$command" =~ ^[A-Za-z0-9][A-Za-z0-9-]*$ ]] || {
    die "invalid Git command name in command-list.txt: $command"
  }
  doc_rel="git-$command.adoc"
  doc_path="$doc_root/$doc_rel"
  [[ -f "$doc_path" && ! -L "$doc_path" ]] || {
    die "missing Git command documentation: $doc_rel"
  }
  canonical_doc="$(canonical_file "$doc_path")" || die "cannot resolve Git command documentation: $doc_rel"
  case "$canonical_doc" in
    "$doc_root"/*) ;;
    *) die "Git command documentation escapes Documentation: $doc_rel" ;;
  esac

  include_stack=()
  stack_depth=0
  export ZMIN_PARSE_COMMAND="$command" ZMIN_PARSE_DOC="$doc_rel"
  parse_doc "$canonical_doc" |
    perl -ne '
      BEGIN {
        $command = $ENV{"ZMIN_PARSE_COMMAND"};
        $doc = $ENV{"ZMIN_PARSE_DOC"};
        $section = "";
        $pending_heading = "";
      }

      chomp;

      if (/^[-=~^+]+$/ && $pending_heading ne "") {
        $section = $pending_heading;
        $pending_heading = "";
        next;
      }

      if (/^[A-Z][A-Z0-9 ()\/-]*$/) {
        $pending_heading = $_;
        next;
      }

      $pending_heading = "";

      next unless $section eq "OPTIONS";
      next unless /^\s*[`+]*-/;
      next unless /::\s*$/;

      s/`//g;
      s/\047//g;

      while (/--\[no-\]([A-Za-z0-9][A-Za-z0-9-]*)/g) {
        print "$command\t--$1\t$doc\n";
        print "$command\t--no-$1\t$doc\n";
      }

      s/--\[no-\][A-Za-z0-9][A-Za-z0-9-]*//g;

      while (/(?<![A-Za-z0-9])(--[A-Za-z0-9][A-Za-z0-9-]*)(?=$|[\s=<>,;:\.\]\[])/g) {
        print "$command\t$1\t$doc\n";
      }

      while (/(?<![A-Za-z0-9])(-[A-Za-z0-9?])(?=$|[\s=<>,;:\.\]\[])/g) {
        print "$command\t$1\t$doc\n";
      }
    '
done <<< "$commands" |
  LC_ALL=C sort -u
