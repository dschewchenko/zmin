#!/usr/bin/env bash
set -euo pipefail

repo_path="${1:-$PWD}"
repeats="${ZMIN_OBSERVED_BENCH_REPEATS:-3}"
warmups="${ZMIN_OBSERVED_BENCH_WARMUPS:-1}"
phase_trace="${ZMIN_OBSERVED_BENCH_PHASE_TRACE:-0}"
stock_git="${ZMIN_STOCK_GIT:-${GIT_BIN:-/usr/bin/git}}"
out_dir="${ZMIN_OBSERVED_BENCH_OUT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/zmin-observed-client-bench.XXXXXX")}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

resolve_repo_target_dir() {
  local cargo_config="$repo_root/.cargo/config.toml"
  if [[ ! -f "$cargo_config" ]]; then
    return 1
  fi
  awk -F '"' '/^[[:space:]]*target-dir[[:space:]]*=[[:space:]]*"/ { print $2; exit }' "$cargo_config"
}

resolve_default_zmin_bin() {
  local target_dir
  target_dir="$(resolve_repo_target_dir || true)"
  if [[ -n "${target_dir:-}" ]]; then
    if [[ -x "$target_dir/release/zmin" ]]; then
      printf '%s\n' "$target_dir/release/zmin"
      return 0
    fi
    if [[ -x "$target_dir/compat/zmin" ]]; then
      printf '%s\n' "$target_dir/compat/zmin"
      return 0
    fi
    if [[ -x "$target_dir/debug/zmin" ]]; then
      printf '%s\n' "$target_dir/debug/zmin"
      return 0
    fi
  fi
  printf '%s\n' "$HOME/.local/bin/git.zmin-bin"
}

zmin_bin="${ZMIN_BIN:-$(resolve_default_zmin_bin)}"

if [[ ! -d "$repo_path/.git" && ! -f "$repo_path/.git" ]]; then
  echo "repo path is not a git worktree: $repo_path" >&2
  exit 1
fi

if [[ ! -x "$stock_git" ]]; then
  echo "stock git is not executable: $stock_git" >&2
  exit 1
fi

if [[ ! -x "$zmin_bin" ]]; then
  echo "zmin binary is not executable: $zmin_bin" >&2
  exit 1
fi

mkdir -p "$out_dir"
summary_tsv="$out_dir/summary.tsv"

cat >"$summary_tsv" <<'EOF'
tool	lane	run	real_seconds	user_seconds	sys_seconds	max_rss_bytes
EOF

run_lane_once() {
  local tool="$1"
  local lane="$2"
  local run="$3"
  local stdin_payload="$4"
  shift 4
  local command=("$@")
  local prefix="$out_dir/${lane}.${tool}.${run}"
  local binary="$stock_git"
  local runner=(python3 "$script_dir/git-bench-process.py")
  if [[ "$tool" == "zmin" ]]; then
    binary="$zmin_bin"
    if [[ "$phase_trace" == "1" ]]; then
      runner=(
        env
        ZMIN_PHASE_TRACE=1
        "ZMIN_PHASE_TRACE_FILE=$prefix.trace"
        python3 "$script_dir/git-bench-process.py"
      )
    fi
  fi
  if [[ "$stdin_payload" != "__ZMIN_NO_STDIN__" ]]; then
    printf '%s\n' "$stdin_payload" >"$prefix.stdin"
  fi
  set +e
  if [[ "$stdin_payload" == "__ZMIN_NO_STDIN__" ]]; then
    "${runner[@]}" \
      --stdout "$prefix.stdout" \
      --stderr "$prefix.stderr" \
      --metrics "$prefix.metrics" \
      -- "$binary" -C "$repo_path" "${command[@]}"
  else
    "${runner[@]}" \
      --stdout "$prefix.stdout" \
      --stderr "$prefix.stderr" \
      --metrics "$prefix.metrics" \
      --stdin "$prefix.stdin" \
      -- "$binary" -C "$repo_path" "${command[@]}"
  fi
  local status=$?
  set -e
  if [[ "$status" != "0" ]]; then
    echo "$tool failed for $lane run $run with exit $status" >&2
    sed -n '1,20p' "$prefix.stderr" >&2
    exit "$status"
  fi
  local metrics
  metrics="$(cat "$prefix.metrics")"
  if [[ "$run" != warmup-* ]]; then
    printf '%s\t%s\t%s\t%s\n' "$tool" "$lane" "$run" "$metrics" >>"$summary_tsv"
  fi
}

run_paired_lane() {
  local lane="$1"
  local stdin_payload="$2"
  shift 2
  local command=("$@")
  for warmup in $(seq 1 "$warmups"); do
    run_lane_once stock "$lane" "warmup-$warmup" "$stdin_payload" "${command[@]}"
    run_lane_once zmin "$lane" "warmup-$warmup" "$stdin_payload" "${command[@]}"
  done
  for run in $(seq 1 "$repeats"); do
    if (( run % 2 == 1 )); then
      run_lane_once stock "$lane" "$run" "$stdin_payload" "${command[@]}"
      run_lane_once zmin "$lane" "$run" "$stdin_payload" "${command[@]}"
    else
      run_lane_once zmin "$lane" "$run" "$stdin_payload" "${command[@]}"
      run_lane_once stock "$lane" "$run" "$stdin_payload" "${command[@]}"
    fi
  done
  compare_outputs "$lane"
}

compare_outputs() {
  local lane="$1"
  for run in $(seq 1 "$repeats"); do
    local stock_prefix="$out_dir/${lane}.stock.${run}"
    local zmin_prefix="$out_dir/${lane}.zmin.${run}"
    if ! cmp -s "$stock_prefix.stdout" "$zmin_prefix.stdout"; then
      echo "stdout mismatch for $lane run $run" >&2
      diff -u "$stock_prefix.stdout" "$zmin_prefix.stdout" >&2 || true
      exit 1
    fi
    if ! cmp -s "$stock_prefix.stderr" "$zmin_prefix.stderr"; then
      echo "stderr mismatch for $lane run $run" >&2
      diff -u "$stock_prefix.stderr" "$zmin_prefix.stderr" >&2 || true
      exit 1
    fi
  done
}

observed_common=(
  -c credential.helper=
  -c core.quotepath=false
  -c log.showSignature=false
)

run_paired_lane observed_status __ZMIN_NO_STDIN__ \
  "${observed_common[@]}" \
  status --porcelain -z --no-renames --untracked-files=all --ignored=matching --

run_paired_lane observed_show_numstat __ZMIN_NO_STDIN__ \
  "${observed_common[@]}" \
  show --numstat --format=%H HEAD

run_paired_lane observed_ls_files_idea __ZMIN_NO_STDIN__ \
  -c core.hooksPath=/dev/null \
  -c core.fsmonitor= \
  ls-files -t --cached --others --exclude-standard -z -- .idea/workspace.xml .idea/workspace.xml~

run_paired_lane observed_show_stdin_name_status $'HEAD\nHEAD~1\n' \
  "${observed_common[@]}" \
  show --name-status "--format=%H %P" --stdin

run_paired_lane observed_rev_parse_repo __ZMIN_NO_STDIN__ \
  rev-parse --show-toplevel --git-dir --is-inside-work-tree

run_paired_lane observed_config_list __ZMIN_NO_STDIN__ \
  config --null --list

run_paired_lane observed_branch_current __ZMIN_NO_STDIN__ \
  branch --show-current

run_paired_lane observed_for_each_ref __ZMIN_NO_STDIN__ \
  for-each-ref --format='%(refname)%00%(objectname)' refs/heads refs/remotes

run_paired_lane observed_ls_tree __ZMIN_NO_STDIN__ \
  ls-tree -r -z --name-only HEAD --

run_paired_lane observed_log_unbounded __ZMIN_NO_STDIN__ \
  "${observed_common[@]}" \
  log \
  --pretty=format:%x01%x01%H%x02%x02%P%x02%x02%ct%x02%x02%an%x02%x02%ae%x02%x02%d%x03%x03 \
  --encoding=UTF-8 \
  --decorate=full \
  HEAD \
  --branches \
  --remotes \
  --tags \
  --date-order \
  --

python3 - "$summary_tsv" <<'PY'
import csv
import math
import os
import statistics
import sys

path = sys.argv[1]
rows = list(csv.DictReader(open(path, newline=""), delimiter="\t"))
lanes = sorted({row["lane"] for row in rows})
print(
    "lane\tstock_median\tzmin_median\ttime_median_ratio\t"
    "stock_p95\tzmin_p95\ttime_p95_ratio\t"
    "stock_rss_p95_bytes\tzmin_rss_p95_bytes\trss_p95_ratio"
)
failures = []
max_time_ratio = float(os.environ.get("ZMIN_OBSERVED_MAX_TIME_P95_RATIO", "0"))
max_rss_ratio = float(os.environ.get("ZMIN_OBSERVED_MAX_RSS_P95_RATIO", "0"))

def percentile(values, percentile):
    ordered = sorted(values)
    index = max(0, math.ceil(len(ordered) * percentile) - 1)
    return ordered[index]

for lane in lanes:
    stock = [float(row["real_seconds"]) for row in rows if row["lane"] == lane and row["tool"] == "stock"]
    zmin = [float(row["real_seconds"]) for row in rows if row["lane"] == lane and row["tool"] == "zmin"]
    stock_rss = [int(row["max_rss_bytes"]) for row in rows if row["lane"] == lane and row["tool"] == "stock"]
    zmin_rss = [int(row["max_rss_bytes"]) for row in rows if row["lane"] == lane and row["tool"] == "zmin"]
    stock_median = statistics.median(stock)
    zmin_median = statistics.median(zmin)
    stock_p95 = percentile(stock, 0.95)
    zmin_p95 = percentile(zmin, 0.95)
    stock_rss_p95 = percentile(stock_rss, 0.95)
    zmin_rss_p95 = percentile(zmin_rss, 0.95)
    median_ratio = zmin_median / stock_median if stock_median else float("inf")
    p95_ratio = zmin_p95 / stock_p95 if stock_p95 else float("inf")
    rss_ratio = zmin_rss_p95 / stock_rss_p95 if stock_rss_p95 else float("inf")
    print(
        f"{lane}\t{stock_median:.6f}\t{zmin_median:.6f}\t{median_ratio:.6f}\t"
        f"{stock_p95:.6f}\t{zmin_p95:.6f}\t{p95_ratio:.6f}\t"
        f"{stock_rss_p95}\t{zmin_rss_p95}\t{rss_ratio:.6f}"
    )
    if max_time_ratio > 0 and p95_ratio > max_time_ratio:
        failures.append(f"{lane}: time p95 ratio {p95_ratio:.6f} > {max_time_ratio:.6f}")
    if max_rss_ratio > 0 and rss_ratio > max_rss_ratio:
        failures.append(f"{lane}: RSS p95 ratio {rss_ratio:.6f} > {max_rss_ratio:.6f}")

if failures:
    raise SystemExit("observed client benchmark gate failed: " + "; ".join(failures))
PY

printf 'observed_client_bench_stock_git=%s\n' "$stock_git"
printf 'observed_client_bench_zmin_bin=%s\n' "$zmin_bin"
printf 'observed_client_bench_zmin_sha256=%s\n' "$(shasum -a 256 "$zmin_bin" | awk '{ print $1 }')"
printf 'observed_client_bench_phase_trace=%s\n' "$phase_trace"
printf 'observed_client_bench_warmups=%s\n' "$warmups"
printf 'observed_client_bench_out=%s\n' "$out_dir"
