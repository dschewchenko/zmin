#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
baseline="${ZMIN_GIT_BASELINE:-v2.47.1}"
cache_dir="${ZMIN_GIT_DOC_CACHE:-$repo_root/target/git-doc-cache/$baseline}"
command_list="${ZMIN_GIT_COMMAND_LIST:-$cache_dir/command-list.txt}"
format="${1:---markdown}"

if [[ ! -f "$command_list" ]]; then
  ZMIN_GIT_BASELINE="$baseline" "$repo_root/tools/git-compat-option-inventory.sh" >/dev/null
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

all_commands="$tmp_dir/all-commands.tsv"
option_seed="$tmp_dir/option-seed.tsv"
matrix_counts="$tmp_dir/matrix-counts.tsv"
represented_options="$tmp_dir/represented-options.tsv"
complete_commands="$repo_root/docs/cli/census/reviewed_complete_command_matrices.tsv"
complete_options="$repo_root/docs/cli/census/reviewed_complete_doc_option_pairs.tsv"

awk '$1 ~ /^git-/ { command = $1; sub(/^git-/, "", command); print command }' "$command_list" |
  sort -u >"$all_commands"

"$repo_root/tools/git-compat-option-inventory.sh" >"$option_seed"

awk -F'\t' '
  FNR == 1 { next }
  {
    command = $2
    rows[command]++
    if ($10 == "closed") closed[command]++
    else if ($10 == "partial") partial[command]++
    else if ($10 == "open") open[command]++
    else if ($10 == "invalid-input") invalid[command]++
  }
  END {
    for (command in rows) {
      print command "\t" rows[command] "\t" closed[command] + 0 "\t" partial[command] + 0 "\t" open[command] + 0 "\t" invalid[command] + 0
    }
  }
' "$repo_root"/docs/cli/matrices/*.tsv | sort >"$matrix_counts"

python3 - "$option_seed" "$repo_root"/docs/cli/matrices/*.tsv >"$represented_options" <<'PY'
import csv
import re
import sys
from collections import defaultdict
from pathlib import Path

LONG_OPTION_PATTERN = re.compile(r"(?<!\S)(--[A-Za-z0-9][A-Za-z0-9-]*)(?:[=\s]|$)")
SHORT_OPTION_PATTERN = re.compile(r"(?<!\S)(-[A-Za-z])(?:[=\s]|$)")

seed_path = Path(sys.argv[1])
matrix_paths = [Path(path) for path in sys.argv[2:]]
seed = set()
with seed_path.open(newline="") as handle:
    for row in csv.DictReader(handle, delimiter="\t"):
        seed.add((row["command"], row["option"]))

represented = set()
for matrix_path in matrix_paths:
    with matrix_path.open(newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            if "stock_git_case" not in row and "example" in row:
                row["stock_git_case"] = row["example"]
            command = row["command"]
            spellings = set()
            if row["option"].startswith("-"):
                spellings.add(row["option"].split("=", 1)[0])
            text = " ".join([row["stock_git_case"], row["combination"]])
            spellings.update(match.group(1) for match in LONG_OPTION_PATTERN.finditer(text))
            spellings.update(match.group(1) for match in SHORT_OPTION_PATTERN.finditer(text))
            for option in spellings:
                if (command, option) in seed:
                    represented.add((command, option))

counts = defaultdict(int)
for command, _option in represented:
    counts[command] += 1

for command in sorted(counts):
    print(f"{command}\t{counts[command]}")
PY

if [[ ! -f "$complete_commands" ]]; then
  printf 'command\tevidence\tnotes\n' >"$complete_commands"
fi

if [[ ! -f "$complete_options" ]]; then
  printf 'command\toption\tevidence\tnotes\n' >"$complete_options"
fi

awk -F'\t' -v format="$format" '
  FILENAME ~ /all-commands/ {
    all[$1] = 1
    command_order[++command_order_count] = $1
    command_count++
    next
  }
  FILENAME ~ /option-seed/ {
    if (FNR > 1) {
      option_seed[$1]++
      seed_pair[$1, $2] = 1
      option_total++
    }
    next
  }
  FILENAME ~ /matrix-counts/ {
    matrix_rows[$1] = $2
    matrix_closed[$1] = $3
    matrix_partial[$1] = $4
    matrix_open[$1] = $5
    matrix_invalid[$1] = $6
    matrix_classified[$1] = ($3 + 0) + ($6 + 0)
    commands_with_matrix[$1] = 1
    matrix_command_count++
    rows_total += $2
    closed_total += $3
    partial_total += $4
    open_total += $5
    invalid_total += $6
    classified_total += ($3 + 0) + ($6 + 0)
    next
  }
  FILENAME ~ /represented-options/ {
    represented[$1] = $2
    represented_total += $2
    next
  }
  FILENAME ~ /complete-options|reviewed_complete_doc_option_pairs/ {
    if (FNR > 1 && $1 != "") {
      if (($1, $2) in seed_pair) {
        complete_options[$1]++
        complete_option_total++
      }
    }
    next
  }
  FILENAME ~ /complete-commands|reviewed_complete_command_matrices/ {
    if (FNR > 1 && $1 != "") {
      complete[$1] = 1
      complete_count++
    }
    next
  }
  END {
    if (format == "--tsv") {
      print "metric\tcount\ttotal\tnote"
      printf "complete_command_matrices\t%d\t%d\tonly commands whose full behavior matrix is finished\n", complete_count + 0, command_count + 0
      printf "complete_doc_option_pairs\t%d\t%d\tdocumented command-option pairs whose full behavior matrix is finished\n", complete_option_total + 0, option_total + 0
      printf "commands_with_matrix_rows\t%d\t%d\tcommands with any written behavior rows\n", matrix_command_count + 0, command_count + 0
      printf "doc_option_pairs_represented_by_rows\t%d\t%d\tdocumented command-option pairs with at least one behavior row\n", represented_total + 0, option_total + 0
      printf "behavior_rows_written\t%d\t%d\tcurrent written command option value combination state transport platform rows\n", rows_total + 0, rows_total + 0
      printf "written_rows_matching_stock_git\t%d\t%d\tclosed written rows only\n", closed_total + 0, rows_total + 0
      printf "behavior_rows_classified\t%d\t%d\tclosed and invalid-input rows safe to skip unless code or evidence changes\n", classified_total + 0, rows_total + 0
      printf "behavior_rows_partial\t%d\t%d\twritten rows with incomplete parity\n", partial_total + 0, rows_total + 0
      printf "behavior_rows_open\t%d\t%d\twritten rows not implemented or not matching yet\n", open_total + 0, rows_total + 0
      printf "invalid_input_rows\t%d\t%d\trows where stock Git rejects the input\n", invalid_total + 0, rows_total + 0
      print ""
      print "command\tdoc_option_pairs\tcomplete_doc_option_pairs\trepresented_doc_option_pairs\trepresented_doc_option_pairs_pct\tbehavior_rows_written\tbehavior_rows_classified\tbehavior_rows_classified_pct_of_written\twritten_rows_matching_stock_git\twritten_rows_matching_stock_git_pct_of_written\tpartial\topen\tinvalid_input\tcomplete_matrix"
      for (i = 1; i <= command_order_count; i++) {
        command = command_order[i]
        if (!(command in commands_with_matrix)) continue
        represented_pct = option_seed[command] > 0 ? (represented[command] + 0) * 100 / option_seed[command] : 0
        classified_pct = matrix_rows[command] > 0 ? (matrix_classified[command] + 0) * 100 / matrix_rows[command] : 0
        verified_pct = matrix_rows[command] > 0 ? (matrix_closed[command] + 0) * 100 / matrix_rows[command] : 0
        printf "%s\t%d\t%d\t%d\t%.1f\t%d\t%d\t%.1f\t%d\t%.1f\t%d\t%d\t%d\t%s\n",
          command, option_seed[command] + 0, complete_options[command] + 0,
          represented[command] + 0, represented_pct,
          matrix_rows[command] + 0, matrix_classified[command] + 0, classified_pct,
          matrix_closed[command] + 0, verified_pct,
          matrix_partial[command] + 0, matrix_open[command] + 0,
          matrix_invalid[command] + 0, (command in complete ? "yes" : "no")
      }
    } else {
      print "| Metric | Count | Meaning |"
      print "| --- | ---: | --- |"
      printf "| Complete command matrices | `%d/%d` | full command behavior matrix finished |\n", complete_count + 0, command_count + 0
      printf "| Complete doc option pairs | `%d/%d` | documented command-option pairs whose full behavior matrix is finished |\n", complete_option_total + 0, option_total + 0
      printf "| Commands with any matrix rows | `%d/%d` | audit has started for the command |\n", matrix_command_count + 0, command_count + 0
      printf "| Doc option pairs represented by rows | `%d/%d` | documented command-option pairs with at least one behavior row |\n", represented_total + 0, option_total + 0
      printf "| Behavior rows written | `%d` | command + option + value + combination + state + transport + platform rows |\n", rows_total + 0
      printf "| Written rows matching stock Git | `%d/%d` | closed written rows only |\n", closed_total + 0, rows_total + 0
      printf "| Classified written rows | `%d/%d` | closed plus invalid-input rows safe to skip unless code or evidence changes |\n", classified_total + 0, rows_total + 0
      printf "| Partial rows | `%d/%d` | written rows with incomplete parity |\n", partial_total + 0, rows_total + 0
      printf "| Open rows | `%d/%d` | written rows not implemented or not matching yet |\n", open_total + 0, rows_total + 0
      printf "| Invalid input rows | `%d/%d` | rows where stock Git rejects the input |\n", invalid_total + 0, rows_total + 0
      print ""
      print "| Command | Git doc option pairs | Complete doc option pairs | Represented doc option pairs | Represented doc option pairs % | Behavior rows written | Classified written rows | Classified written rows % | Written rows matching stock Git | Verified written rows % | Partial | Open | Invalid input | Complete matrix |"
      print "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |"
      for (i = 1; i <= command_order_count; i++) {
        command = command_order[i]
        if (!(command in commands_with_matrix)) continue
        represented_pct = option_seed[command] > 0 ? (represented[command] + 0) * 100 / option_seed[command] : 0
        classified_pct = matrix_rows[command] > 0 ? (matrix_classified[command] + 0) * 100 / matrix_rows[command] : 0
        verified_pct = matrix_rows[command] > 0 ? (matrix_closed[command] + 0) * 100 / matrix_rows[command] : 0
        printf "| `%s` | `%d` | `%d` | `%d` | `%.1f%%` | `%d` | `%d` | `%.1f%%` | `%d` | `%.1f%%` | `%d` | `%d` | `%d` | %s |\n",
          command, option_seed[command] + 0, complete_options[command] + 0,
          represented[command] + 0, represented_pct,
          matrix_rows[command] + 0, matrix_classified[command] + 0, classified_pct,
          matrix_closed[command] + 0, verified_pct,
          matrix_partial[command] + 0, matrix_open[command] + 0,
          matrix_invalid[command] + 0, (command in complete ? "yes" : "no")
      }
    }
  }
' "$all_commands" "$option_seed" "$matrix_counts" "$represented_options" "$complete_options" "$complete_commands"
