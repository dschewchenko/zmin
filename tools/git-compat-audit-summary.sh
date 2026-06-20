#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
baseline="${ZMIN_GIT_BASELINE:-v2.47.1}"
cache_dir="${ZMIN_GIT_DOC_CACHE:-$repo_root/target/git-doc-cache/$baseline}"
command_list="${ZMIN_GIT_COMMAND_LIST:-$cache_dir/command-list.txt}"
groups_file="${ZMIN_GIT_REFERENCE_GROUPS:-$repo_root/docs/cli/git_reference_groups.tsv}"
primary_groups_file="${ZMIN_GIT_AUDIT_PRIMARY_GROUPS:-$repo_root/docs/cli/git_audit_primary_groups.tsv}"
variant_plan_file="$repo_root/docs/cli/variant_compatibility_plan.md"
format="${1:---markdown}"

if [[ ! -f "$command_list" ]]; then
  ZMIN_GIT_BASELINE="$baseline" "$repo_root/tools/git-compat-option-inventory.sh" >/dev/null
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

all_commands="$tmp_dir/all-commands.tsv"
option_seed="$tmp_dir/option-seed.tsv"
matrix_counts="$tmp_dir/matrix-counts.tsv"
closed_blocks="$tmp_dir/closed-blocks.tsv"
unique_totals="$tmp_dir/unique-totals.tsv"
primary_map="$tmp_dir/primary-groups.tsv"

awk '$1 ~ /^git-/ { command = $1; sub(/^git-/, "", command); print command }' "$command_list" |
  sort -u >"$all_commands"

"$repo_root/tools/git-compat-option-inventory.sh" >"$option_seed"

awk -F'\t' '
  NR == FNR {
    if (FNR > 1) {
      group_for[$2] = group_for[$2] ? group_for[$2] SUBSEP $1 : $1
      explicit[$2] = 1
      groups[$1] = 1
    }
    next
  }
  {
    command = $0
    if (command in group_for) {
      n = split(group_for[command], memberships, SUBSEP)
      for (i = 1; i <= n; i++) {
        group = memberships[i]
        group_commands[group, command] = 1
      }
    } else {
      group_commands["Other Git 2.47 commands", command] = 1
    }
  }
  END {
    for (key in group_commands) {
      split(key, parts, SUBSEP)
      count[parts[1]]++
    }
    for (group in count) {
      print group "\t" count[group]
    }
  }
' "$groups_file" "$all_commands" | sort >"$tmp_dir/group-command-counts.tsv"

awk -F'\t' '
  NR == FNR {
    if (FNR > 1) {
      group_for[$2] = group_for[$2] ? group_for[$2] SUBSEP $1 : $1
    }
    next
  }
  FNR == 1 { next }
  {
    command = $1
    if (command in group_for) {
      n = split(group_for[command], memberships, SUBSEP)
      for (i = 1; i <= n; i++) option_rows[memberships[i]]++
    } else {
      option_rows["Other Git 2.47 commands"]++
    }
  }
  END {
    for (group in option_rows) {
      print group "\t" option_rows[group]
    }
  }
' "$groups_file" "$option_seed" | sort >"$tmp_dir/group-option-counts.tsv"

awk -F'\t' '
  FNR == 1 { next }
  {
    group = $1
    rows[group]++
    if ($10 == "closed") closed[group]++
    else if ($10 == "partial") partial[group]++
    else if ($10 == "open") open[group]++
    else if ($10 == "invalid-input") invalid[group]++
  }
  END {
    for (group in rows) {
      print group "\t" rows[group] "\t" closed[group] + 0 "\t" partial[group] + 0 "\t" open[group] + 0 "\t" invalid[group] + 0
    }
  }
' "$repo_root"/docs/cli/matrices/*.tsv | sort >"$matrix_counts"

awk -F'\t' '
  NR == FNR {
    if (FNR > 1 && !($2 in primary)) primary[$2] = $1
    next
  }
  FNR > 1 {
    primary[$1] = $2
  }
  END {
    for (command in primary) {
      print command "\t" primary[command]
    }
  }
' "$groups_file" "$primary_groups_file" >"$primary_map"

awk -F'\t' '
  NR == FNR {
    primary[$1] = $2
    next
  }
  /^## Closed Evidence Blocks/ { active = 1; next }
  /^Tracked closed/ { active = 0 }
  active && /^\| `[^`]+`/ {
    split($0, columns, "|")
    command = columns[2]
    count = columns[3]
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", command)
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", count)
    gsub(/`/, "", command)
    gsub(/`/, "", count)
    sub(/ .*/, "", command)
    count += 0
    if (count > 0) {
      group = command in primary ? primary[command] : "Other Git 2.47 commands"
      closed[group] += count
      total += count
    }
  }
  END {
    for (group in closed) {
      print group "\t" closed[group]
    }
    print "closed_block_total\t" total > totals_file
  }
' totals_file="$unique_totals" "$primary_map" "$variant_plan_file" | sort >"$closed_blocks"

{
  printf 'git_command_total\t'
  wc -l <"$all_commands" | tr -d ' '
  printf 'git_doc_option_seed_total\t'
  awk 'NR > 1 { count++ } END { print count + 0 }' "$option_seed"
  awk -F'\t' '
    { rows += $2; closed += $3; partial += $4; open += $5; invalid += $6 }
    END {
      print "matrix_rows_total\t" rows + 0
      print "matrix_closed_total\t" closed + 0
      print "matrix_partial_total\t" partial + 0
      print "matrix_open_total\t" open + 0
      print "matrix_invalid_total\t" invalid + 0
    }
  ' "$matrix_counts"
  cat "$unique_totals"
} >"$tmp_dir/unique-summary.tsv"

if [[ "$format" == "--tsv" ]]; then
  printf 'group\tgit_commands\tgit_doc_option_seed_rows\tmatrix_rows\tmatrix_matching_stock_git\tmatrix_partial\tmatrix_open\tmatrix_invalid_input\tclosed_block_variants\n'
else
  printf '| Git reference group | Git commands | Git doc option seed rows | Matrix rows | Matrix rows matching stock Git | Matrix partial | Matrix open | Matrix invalid input | Closed block variants |\n'
  printf '| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n'
fi

awk -F'\t' -v format="$format" '
  FILENAME ~ /group-command-counts/ { commands[$1] = $2; groups[$1] = 1; next }
  FILENAME ~ /group-option-counts/ { options[$1] = $2; groups[$1] = 1; next }
  FILENAME ~ /matrix-counts/ {
    matrix_rows[$1] = $2
    matrix_closed[$1] = $3
    matrix_partial[$1] = $4
    matrix_open[$1] = $5
    matrix_invalid[$1] = $6
    groups[$1] = 1
    next
  }
  FILENAME ~ /closed-blocks/ { ad_hoc[$1] = $2; groups[$1] = 1; next }
  FILENAME ~ /unique-summary/ { unique[$1] = $2; next }
  END {
    order[1] = "Setup and Config"
    order[2] = "Getting and Creating Projects"
    order[3] = "Basic Snapshotting"
    order[4] = "Branching and Merging"
    order[5] = "Sharing and Updating Projects"
    order[6] = "Inspection and Comparison"
    order[7] = "Patching"
    order[8] = "Debugging"
    order[9] = "Email"
    order[10] = "External Systems"
    order[11] = "Administration"
    order[12] = "Server Admin"
    order[13] = "Plumbing Commands"
    order[14] = "Other Git 2.47 commands"
    for (i = 1; i <= 14; i++) {
      group = order[i]
      if (format == "--tsv") {
        printf "%s\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\n",
          group, commands[group] + 0, options[group] + 0, matrix_rows[group] + 0,
          matrix_closed[group] + 0, matrix_partial[group] + 0, matrix_open[group] + 0,
          matrix_invalid[group] + 0, ad_hoc[group] + 0
      } else {
        printf "| %s | `%d` | `%d` | `%d` | `%d` | `%d` | `%d` | `%d` | `%d` |\n",
          group, commands[group] + 0, options[group] + 0, matrix_rows[group] + 0,
          matrix_closed[group] + 0, matrix_partial[group] + 0, matrix_open[group] + 0,
          matrix_invalid[group] + 0, ad_hoc[group] + 0
      }
    }
    if (format == "--tsv") {
      printf "Git 2.47 unique total\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\n",
        unique["git_command_total"] + 0, unique["git_doc_option_seed_total"] + 0,
        unique["matrix_rows_total"] + 0, unique["matrix_closed_total"] + 0,
        unique["matrix_partial_total"] + 0, unique["matrix_open_total"] + 0,
        unique["matrix_invalid_total"] + 0, unique["closed_block_total"] + 0
    } else {
      printf "| **Git 2.47 unique total** | **`%d`** | **`%d`** | **`%d`** | **`%d`** | **`%d`** | **`%d`** | **`%d`** | **`%d`** |\n",
        unique["git_command_total"] + 0, unique["git_doc_option_seed_total"] + 0,
        unique["matrix_rows_total"] + 0, unique["matrix_closed_total"] + 0,
        unique["matrix_partial_total"] + 0, unique["matrix_open_total"] + 0,
        unique["matrix_invalid_total"] + 0, unique["closed_block_total"] + 0
    }
  }
' "$tmp_dir/group-command-counts.tsv" "$tmp_dir/group-option-counts.tsv" "$matrix_counts" "$closed_blocks" "$tmp_dir/unique-summary.tsv"
