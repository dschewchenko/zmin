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
complete_commands="$tmp_dir/complete-commands.tsv"
complete_options="$tmp_dir/complete-options.tsv"

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

awk -F'\t' '
  NR == FNR {
    if (FNR > 1) seed[$1, $2] = 1
    next
  }
  FNR == 1 { next }
  {
    command = $2
    option = $3
    if ((command, option) in seed) represented[command, option] = 1
  }
  END {
    for (key in represented) {
      split(key, parts, SUBSEP)
      count[parts[1]]++
    }
    for (command in count) print command "\t" count[command]
  }
' "$option_seed" "$repo_root"/docs/cli/matrices/*.tsv | sort >"$represented_options"

# A command becomes complete only after the command-specific matrix has every
# documented and discovered behavior row with stock-Git evidence. Keep this
# file empty until a command has been reviewed against that rule.
: >"$complete_commands"

# A documented command-option pair becomes complete only after all values,
# negations, repeated forms, ordering, repository states, transports and
# platforms for that pair have stock-Git evidence. Keep this file empty until
# a command-option pair has been reviewed against that rule.
: >"$complete_options"

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
    commands_with_matrix[$1] = 1
    matrix_command_count++
    rows_total += $2
    closed_total += $3
    partial_total += $4
    open_total += $5
    invalid_total += $6
    next
  }
  FILENAME ~ /represented-options/ {
    represented[$1] = $2
    represented_total += $2
    next
  }
  FILENAME ~ /complete-options/ {
    if ($1 != "") {
      complete_options[$1]++
      complete_option_total++
    }
    next
  }
  FILENAME ~ /complete-commands/ {
    if ($1 != "") {
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
      printf "behavior_rows_partial\t%d\t%d\twritten rows with incomplete parity\n", partial_total + 0, rows_total + 0
      printf "behavior_rows_open\t%d\t%d\twritten rows not implemented or not matching yet\n", open_total + 0, rows_total + 0
      printf "invalid_input_rows\t%d\t%d\trows where stock Git rejects the input\n", invalid_total + 0, rows_total + 0
      print ""
      print "command\tdoc_option_pairs\tcomplete_doc_option_pairs\trepresented_doc_option_pairs\tbehavior_rows_written\twritten_rows_matching_stock_git\tpartial\topen\tinvalid_input\tcomplete_matrix"
      for (i = 1; i <= command_order_count; i++) {
        command = command_order[i]
        if (!(command in commands_with_matrix)) continue
        printf "%s\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%s\n",
          command, option_seed[command] + 0, complete_options[command] + 0,
          represented[command] + 0,
          matrix_rows[command] + 0, matrix_closed[command] + 0,
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
      printf "| Partial rows | `%d/%d` | written rows with incomplete parity |\n", partial_total + 0, rows_total + 0
      printf "| Open rows | `%d/%d` | written rows not implemented or not matching yet |\n", open_total + 0, rows_total + 0
      printf "| Invalid input rows | `%d/%d` | rows where stock Git rejects the input |\n", invalid_total + 0, rows_total + 0
      print ""
      print "| Command | Git doc option pairs | Complete doc option pairs | Represented doc option pairs | Behavior rows written | Written rows matching stock Git | Partial | Open | Invalid input | Complete matrix |"
      print "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |"
      for (i = 1; i <= command_order_count; i++) {
        command = command_order[i]
        if (!(command in commands_with_matrix)) continue
        printf "| `%s` | `%d` | `%d` | `%d` | `%d` | `%d` | `%d` | `%d` | `%d` | %s |\n",
          command, option_seed[command] + 0, complete_options[command] + 0,
          represented[command] + 0,
          matrix_rows[command] + 0, matrix_closed[command] + 0,
          matrix_partial[command] + 0, matrix_open[command] + 0,
          matrix_invalid[command] + 0, (command in complete ? "yes" : "no")
      }
    }
  }
' "$all_commands" "$option_seed" "$matrix_counts" "$represented_options" "$complete_options" "$complete_commands"
