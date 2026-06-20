#!/usr/bin/env bash
set -euo pipefail

baseline="${ZMIN_GIT_BASELINE:-v2.47.1}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cache_dir="${ZMIN_GIT_DOC_CACHE:-$repo_root/target/git-doc-cache/$baseline}"
command_list="${ZMIN_GIT_COMMAND_LIST:-$cache_dir/command-list.txt}"

mkdir -p "$cache_dir"

if [[ ! -f "$command_list" ]]; then
  curl -fsSL "https://raw.githubusercontent.com/git/git/${baseline}/command-list.txt" \
    -o "$command_list"
fi

seen_includes=$'\n'

fetch_doc() {
  local doc_path="$1"
  local output="$cache_dir/$doc_path"
  local output_dir

  while [[ "$doc_path" == ../* ]]; do
    doc_path="${doc_path#../}"
  done

  output="$cache_dir/$doc_path"
  output_dir="$(dirname "$output")"
  mkdir -p "$output_dir"

  if [[ ! -f "$output" ]]; then
    if ! curl -fsSL "https://raw.githubusercontent.com/git/git/${baseline}/Documentation/${doc_path}" 2>/dev/null \
      -o "$output"; then
      rm -f "$output"
      return 1
    fi
  fi

  printf '%s\n' "$output"
}

emit_doc_with_includes() {
  local doc_path="$1"
  local file line include_path

  while [[ "$doc_path" == ../* ]]; do
    doc_path="${doc_path#../}"
  done

  if [[ "$seen_includes" == *$'\n'"$doc_path"$'\n'* ]]; then
    return
  fi
  seen_includes+="$doc_path"$'\n'

  if ! file="$(fetch_doc "$doc_path")"; then
    return
  fi
  while IFS= read -r line; do
    if [[ "$line" =~ ^include::([^[]+)\[\] ]]; then
      include_path="${BASH_REMATCH[1]}"
      if [[ "$include_path" != /* ]]; then
        emit_doc_with_includes "$include_path"
      fi
    else
      printf '%s\n' "$line"
    fi
  done <"$file"
}

printf 'command\toption\tdoc\n'

awk '$1 ~ /^git-/ { command = $1; sub(/^git-/, "", command); print command }' "$command_list" |
  sort -u |
  while IFS= read -r command; do
    doc="git-$command.txt"
    seen_includes=$'\n'

    if ! fetch_doc "$doc" >/dev/null 2>&1; then
      rm -f "$cache_dir/$doc"
      continue
    fi

    ZMIN_PARSE_COMMAND="$command"
    ZMIN_PARSE_DOC="$doc"
    export ZMIN_PARSE_COMMAND ZMIN_PARSE_DOC

    emit_doc_with_includes "$doc" |
      perl -ne '
        BEGIN {
          $command = $ENV{"ZMIN_PARSE_COMMAND"};
          $doc = $ENV{"ZMIN_PARSE_DOC"};
        }

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
  done |
  sort -u
