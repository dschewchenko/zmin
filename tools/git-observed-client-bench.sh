#!/usr/bin/env bash
set -euo pipefail

repo_path="${1:-$PWD}"
evidence_mode="${ZMIN_OBSERVED_BENCH_EVIDENCE_MODE:-exploratory}"
if [[ "$evidence_mode" == "authoritative" ]]; then
  repeats="${ZMIN_OBSERVED_BENCH_REPEATS:-30}"
  warmups="${ZMIN_OBSERVED_BENCH_WARMUPS:-3}"
  cold_starts="${ZMIN_OBSERVED_BENCH_COLD_STARTS:-10}"
else
  repeats="${ZMIN_OBSERVED_BENCH_REPEATS:-3}"
  warmups="${ZMIN_OBSERVED_BENCH_WARMUPS:-1}"
  cold_starts="${ZMIN_OBSERVED_BENCH_COLD_STARTS:-0}"
fi
phase_trace="${ZMIN_OBSERVED_BENCH_PHASE_TRACE:-}"
stock_git="${ZMIN_STOCK_GIT:-${GIT_BIN:-}}"
out_dir="${ZMIN_OBSERVED_BENCH_OUT_DIR:-}"
out_dir_explicit=0
if [[ -n "$out_dir" ]]; then
  out_dir_explicit=1
fi
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
observed_lane_manifest="observed_status,observed_show_numstat,observed_ls_files_idea,observed_show_stdin_name_status,observed_rev_parse_repo,observed_config_list,observed_branch_current,observed_for_each_ref,observed_ls_tree,observed_log_unbounded"
source "$repo_root/tools/benchmark-environment.sh"
benchmark_authoritative_trace_preflight "$evidence_mode" "$phase_trace"
python_bin="$(benchmark_resolve_python "$evidence_mode")"

artifact_cli() {
  "$python_bin" "$repo_root/tools/performance_contract.py" "$@"
}

artifact_preflight_root() {
  local root="$1"; shift
  artifact_cli artifact-preflight --root "$root" "$@"
}

artifact_write_root() {
  local root="$1" identity="$2" name="$3" data="$4"
  printf '%s' "$data" | artifact_cli artifact-write \
    --root "$root" --root-identity "$identity" --name "$name"
}

artifact_append_root() {
  local root="$1" identity="$2" name="$3" data="$4"
  printf '%s' "$data" | artifact_cli artifact-write \
    --root "$root" --root-identity "$identity" --name "$name" --append
}

artifact_read_root() {
  local root="$1" identity="$2" name="$3"
  artifact_cli artifact-read --root "$root" --root-identity "$identity" --name "$name"
}

artifact_copy_root() {
  local root="$1" identity="$2" name="$3" source="$4"
  artifact_cli artifact-copy \
    --root "$root" --root-identity "$identity" --name "$name" --source "$source"
}

if [[ "$evidence_mode" == "authoritative" && -z "${ZMIN_BIN:-}" ]]; then
  printf 'authoritative observed runs require an explicit prebuilt ZMIN_BIN release binary\n' >&2
  exit 1
fi
if [[ "$evidence_mode" == "authoritative" && -z "${ZMIN_STOCK_GIT:-}" ]]; then
  printf 'authoritative observed runs require an explicit pinned ZMIN_STOCK_GIT\n' >&2
  exit 1
fi

stock_git="$(benchmark_resolve_observed_git)"

export GIT_CONFIG_NOSYSTEM="${GIT_CONFIG_NOSYSTEM:-1}"
export GIT_CONFIG_GLOBAL="${GIT_CONFIG_GLOBAL:-/dev/null}"

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
zmin_bin="$(cd "$(dirname "$zmin_bin")" && pwd)/$(basename "$zmin_bin")"
stock_git="$(cd "$(dirname "$stock_git")" && pwd)/$(basename "$stock_git")"
comparator_bundle=""
if [[ "$evidence_mode" == "authoritative" ]]; then
  comparator_bundle="$(benchmark_validate_authoritative_git_comparator "$repo_root" "$stock_git")" || exit 1
fi
zmin_profile="unknown"
case "$zmin_bin" in
  */release/*) zmin_profile=release ;;
  */compat/*) zmin_profile=compat ;;
  */debug/*) zmin_profile=debug ;;
esac

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

sandbox_dir="$(mktemp -d /tmp/zmin-observed-env.XXXXXX)"
stream_dir=""
cleanup() {
  rm -rf "$sandbox_dir"
  if [[ -n "${stream_dir:-}" ]]; then
    rm -rf "$stream_dir"
  fi
}
trap cleanup EXIT
benchmark_sanitize_environment "$sandbox_dir" "$stock_git" "$zmin_bin" "$python_bin"
if [[ "$evidence_mode" == "authoritative" ]]; then
  export ZMIN_BENCH_GIT_COMPARATOR_STATUS=validated
  export ZMIN_BENCH_GIT_COMPARATOR_BUNDLE="$comparator_bundle"
  export ZMIN_BENCH_GIT_COMPARATOR_CONTRACT='v2.55.0;e9019fcafe0040228b8631c30f97ae1adb61bcdc;72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49'
fi
if [[ "$evidence_mode" == "authoritative" && -z "${ZMIN_OBSERVED_BENCH_MAKE:-}" ]]; then
  printf 'authoritative observed benchmark requires explicit ZMIN_OBSERVED_BENCH_MAKE\n' >&2
  exit 1
fi
make_bin="${ZMIN_OBSERVED_BENCH_MAKE:-$(command -v make)}"
make_bin="$(cd "$(dirname "$make_bin")" && pwd -P)/$(basename "$make_bin")"
[[ -x "$make_bin" && ! -L "$make_bin" ]] || {
  printf 'observed benchmark Make binary is not a canonical executable: %s\n' "$make_bin" >&2
  exit 1
}
if [[ "$evidence_mode" == "authoritative" && ( "$out_dir_explicit" != "1" || "$out_dir" != /* ) ]]; then
  printf 'authoritative observed runs require an explicit absolute ZMIN_OBSERVED_BENCH_OUT_DIR\n' >&2
  exit 1
fi
if [[ -z "$out_dir" ]]; then
  out_dir="$(mktemp -d /tmp/zmin-observed-client-bench.XXXXXX)"
fi
prepare_results_args=(
  --path "$out_dir"
)
if [[ "$evidence_mode" == "authoritative" ]]; then
  prepare_results_args+=(--require-existing)
fi
out_dir="$(
  "$python_bin" "$repo_root/tools/performance_contract.py" prepare-results-dir \
    "${prepare_results_args[@]}"
)"
artifact_identity="$(artifact_preflight_root "$out_dir" \
  --name summary.tsv --name metadata.json --name evidence.json --name superiority.tsv \
  --name equivalence.tsv)"

stream_dir="$(mktemp -d /tmp/zmin-observed-client-streams.XXXXXX)"
stream_identity="$(artifact_preflight_root "$stream_dir" --name .zmin-stream-root)"

summary_tsv="$out_dir/summary.tsv"

artifact_write_root "$out_dir" "$artifact_identity" summary.tsv \
  $'tool\tlane\tsample_kind\tpair_id\torder_index\trun\treal_seconds\tuser_seconds\tsys_seconds\tmax_rss_bytes\tjob_commit_bytes\tmajor_page_faults\tminor_page_faults\tread_bytes\twrite_bytes\tmemory_metric\tmemory_semantics\tmemory_scope\tmemory_unit\tmetrics_availability\texit\n'
artifact_write_root "$out_dir" "$artifact_identity" equivalence.tsv \
  $'lane\tsample_kind\tphase\tpair_id\tpair_index\tgit_order_index\tzmin_order_index\tgit_exit\tzmin_exit\tgit_stdout_sha256\tzmin_stdout_sha256\tgit_stderr_sha256\tzmin_stderr_sha256\texit_equal\tstdout_equal\tstderr_equal\n'

metadata_path="$out_dir/metadata.json"
fixture_root="$(cd "$repo_path" && pwd)"
command_corpus=observed-client-bench-v3:observed
fixture_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" fixture-hash --root "$fixture_root" --git-bin "$stock_git")"
stock_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$stock_git")"
zmin_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$zmin_bin")"
stock_version="$($stock_git --version)"
zmin_version="$($zmin_bin --version)"
python_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$python_bin")"
python_version="$("$python_bin" --version 2>&1)"
make_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$make_bin")"
make_version="$("$make_bin" --version 2>&1 | sed -n '1p')"
sidecar_path="$zmin_bin.identity.json"
if [[ -f "$sidecar_path" ]]; then
  sidecar_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$sidecar_path")"
else
  sidecar_sha256=missing
fi
harness_paths=(
  "$script_dir/git-observed-client-bench.sh"
  "$script_dir/git-bench-process.py"
  "$script_dir/benchmark-environment.sh"
  "$script_dir/performance_contract.py"
)
start_harness_args=()
finish_anchor_args=(
  --anchor-repo-root "$repo_root"
  --anchor-fixture-root "$fixture_root"
  --anchor-command-corpus "$command_corpus"
  --anchor-fixture-sha256 "$fixture_sha256"
  --anchor-git-bin "$stock_git"
  --anchor-git-sha256 "$stock_sha256"
  --anchor-git-version "$stock_version"
  --anchor-zmin-bin "$zmin_bin"
  --anchor-zmin-sha256 "$zmin_sha256"
  --anchor-zmin-version "$zmin_version"
  --anchor-python-bin "$python_bin"
  --anchor-python-sha256 "$python_sha256"
  --anchor-python-version "$python_version"
  --anchor-make-bin "$make_bin"
  --anchor-make-sha256 "$make_sha256"
  --anchor-make-version "$make_version"
  --anchor-build-profile "$zmin_profile"
  --anchor-identity-sidecar "$sidecar_path"
  --anchor-identity-sidecar-sha256 "$sidecar_sha256"
)
for harness_path in "${harness_paths[@]}"; do
  harness_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$harness_path")"
  start_harness_args+=(--harness "$harness_path")
  finish_anchor_args+=(--anchor-harness "$harness_path" --anchor-harness-sha256 "$harness_sha256")
done
"$python_bin" "$repo_root/tools/performance_contract.py" start \
  --repo-root "$repo_root" \
  --git-bin "$stock_git" \
  --zmin-bin "$zmin_bin" \
  --python-bin "$python_bin" \
  --make-bin "$make_bin" \
  --fixture-root "$fixture_root" \
  --results-dir "$out_dir" \
  --output "$metadata_path" \
  --mode "$evidence_mode" \
  --build-profile "$zmin_profile" \
  --command-corpus "$command_corpus" \
  --mandatory-manifest observed \
  --mandatory-lanes "$observed_lane_manifest" \
  --warmups "$warmups" \
  --measured-pairs "$repeats" \
  --cold-starts "$cold_starts" \
  --ordering interleaved-paired \
  --seed "${ZMIN_OBSERVED_BENCH_SEED:-1900000000}" \
  --equivalence-manifest equivalence.tsv \
  "${start_harness_args[@]}"
start_metadata_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$metadata_path")"
finish_anchor_args+=(--anchor-start-metadata-sha256 "$start_metadata_sha256")

reset_pair_equivalence() {
  pair_lane=""
  pair_sample_kind=""
  pair_id=""
  pair_git_exit=""
  pair_git_order=""
  pair_git_stdout_sha256=""
  pair_git_stderr_sha256=""
  pair_git_stdout_file=""
  pair_git_stderr_file=""
  pair_zmin_exit=""
  pair_zmin_order=""
  pair_zmin_stdout_sha256=""
  pair_zmin_stderr_sha256=""
  pair_zmin_stdout_file=""
  pair_zmin_stderr_file=""
}

retain_stream_file() {
  local source="$1"
  if [[ -f "$source" ]]; then
    artifact_copy_root "$out_dir" "$artifact_identity" "$(basename "$source")" "$source"
  fi
}

retain_pair_streams() {
  retain_stream_file "$pair_git_stdout_file"
  retain_stream_file "$pair_git_stderr_file"
  retain_stream_file "$pair_zmin_stdout_file"
  retain_stream_file "$pair_zmin_stderr_file"
}

cleanup_pair_streams() {
  local stdout_file prefix
  for stdout_file in "$pair_git_stdout_file" "$pair_zmin_stdout_file"; do
    prefix="${stdout_file%.stdout}"
    rm -f -- \
      "$prefix.stdout" "$prefix.stderr" "$prefix.metrics" \
      "$prefix.stdin" "$prefix.trace"
  done
}

record_pair_equivalence() {
  local exit_equal=false stdout_equal=false stderr_equal=false
  [[ "$pair_git_exit" == "$pair_zmin_exit" ]] && exit_equal=true
  [[ "$pair_git_stdout_sha256" == "$pair_zmin_stdout_sha256" ]] && stdout_equal=true
  [[ "$pair_git_stderr_sha256" == "$pair_zmin_stderr_sha256" ]] && stderr_equal=true
  local phase="$pair_sample_kind"
  [[ "$pair_sample_kind" == cold ]] && phase=process-cold
  local equivalence_row="$pair_lane"$'\t'"$pair_sample_kind"$'\t'"$phase"$'\t'"$pair_id"$'\t'"${pair_id##*-}"$'\t'"$pair_git_order"$'\t'"$pair_zmin_order"$'\t'"$pair_git_exit"$'\t'"$pair_zmin_exit"$'\t'"$pair_git_stdout_sha256"$'\t'"$pair_zmin_stdout_sha256"$'\t'"$pair_git_stderr_sha256"$'\t'"$pair_zmin_stderr_sha256"$'\t'"$exit_equal"$'\t'"$stdout_equal"$'\t'"$stderr_equal"$'\n'
  artifact_append_root "$out_dir" "$artifact_identity" equivalence.tsv \
    "$equivalence_row"
  if [[ "$exit_equal" != true || "$stdout_equal" != true || "$stderr_equal" != true ]]; then
    retain_pair_streams
    printf 'Git/Zmin output equivalence mismatch for %s (%s)\n' "$pair_id" "$phase" >&2
    exit 1
  fi
  cleanup_pair_streams
}

run_lane_once() {
  local tool="$1"
  local lane="$2"
  local sample_kind="$3"
  local run="$4"
  local pair_id="$5"
  local order_index="$6"
  local stdin_payload="$7"
  shift 7
  local command=("$@")
  local prefix="$stream_dir/${lane}.${tool}.${sample_kind}.${run}"
  local binary="$stock_git"
  local runner=("$python_bin" "$script_dir/git-bench-process.py")
  if [[ "$tool" == "zmin" ]]; then
    binary="$zmin_bin"
    if [[ "$phase_trace" == "1" ]]; then
      runner=(
        env
        ZMIN_PHASE_TRACE=1
        "ZMIN_PHASE_TRACE_FILE=$prefix.trace"
        "$python_bin" "$script_dir/git-bench-process.py"
      )
    fi
  fi
  if [[ "$stdin_payload" != "__ZMIN_NO_STDIN__" ]]; then
    artifact_write_root "$stream_dir" "$stream_identity" "$(basename "$prefix.stdin")" \
      "$stdin_payload$(printf '\n')"
  fi
  set +e
  if [[ "$stdin_payload" == "__ZMIN_NO_STDIN__" ]]; then
    "${runner[@]}" \
      --artifact-root "$stream_dir" --artifact-root-identity "$stream_identity" \
      --stdout "$prefix.stdout" \
      --stderr "$prefix.stderr" \
      --metrics "$prefix.metrics" \
      -- "$binary" -C "$repo_path" "${command[@]}"
  else
    "${runner[@]}" \
      --artifact-root "$stream_dir" --artifact-root-identity "$stream_identity" \
      --stdout "$prefix.stdout" \
      --stderr "$prefix.stderr" \
      --metrics "$prefix.metrics" \
      --stdin "$prefix.stdin" \
      -- "$binary" -C "$repo_path" "${command[@]}"
  fi
  local status=$?
  set -e
  if [[ "$status" != "0" ]]; then
    retain_stream_file "$prefix.stdout"
    retain_stream_file "$prefix.stderr"
    echo "$tool failed for $lane run $run with exit $status" >&2
    artifact_read_root "$stream_dir" "$stream_identity" "$(basename "$prefix.stderr")" | sed -n '1,20p' >&2
    exit "$status"
  fi
  local metrics
  metrics="$(artifact_read_root "$stream_dir" "$stream_identity" "$(basename "$prefix.metrics")")"
  local row="$tool"$'\t'"$lane"$'\t'"$sample_kind"$'\t'"$pair_id"$'\t'"$order_index"$'\t'"$run"$'\t'"$metrics"$'\t'"$status"
  row+=$'\n'
  artifact_append_root "$out_dir" "$artifact_identity" summary.tsv \
    "$row"
  if [[ "$tool" == stock ]]; then
    pair_git_exit="$status"
    pair_git_order="$order_index"
    pair_git_stdout_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$prefix.stdout")"
    pair_git_stderr_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$prefix.stderr")"
    pair_git_stdout_file="$prefix.stdout"
    pair_git_stderr_file="$prefix.stderr"
  else
    pair_zmin_exit="$status"
    pair_zmin_order="$order_index"
    pair_zmin_stdout_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$prefix.stdout")"
    pair_zmin_stderr_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$prefix.stderr")"
    pair_zmin_stdout_file="$prefix.stdout"
    pair_zmin_stderr_file="$prefix.stderr"
  fi
}

run_paired_lane() {
  local lane="$1"
  local stdin_payload="$2"
  shift 2
  local command=("$@")
  if (( warmups > 0 )); then
    for warmup in $(seq 1 "$warmups"); do
      pair_id="$lane-warmup-$warmup"
      reset_pair_equivalence
      pair_lane="$lane"
      pair_sample_kind=warmup
      run_lane_once stock "$lane" warmup "$warmup" "$pair_id" 1 "$stdin_payload" "${command[@]}"
      run_lane_once zmin "$lane" warmup "$warmup" "$pair_id" 2 "$stdin_payload" "${command[@]}"
      record_pair_equivalence
    done
  fi
  for run in $(seq 1 "$repeats"); do
    pair_id="$lane-measured-$run"
    reset_pair_equivalence
    pair_lane="$lane"
    pair_sample_kind=measured
    if (( run % 2 == 1 )); then
      run_lane_once stock "$lane" measured "$run" "$pair_id" 1 "$stdin_payload" "${command[@]}"
      run_lane_once zmin "$lane" measured "$run" "$pair_id" 2 "$stdin_payload" "${command[@]}"
    else
      run_lane_once zmin "$lane" measured "$run" "$pair_id" 1 "$stdin_payload" "${command[@]}"
      run_lane_once stock "$lane" measured "$run" "$pair_id" 2 "$stdin_payload" "${command[@]}"
    fi
    record_pair_equivalence
  done
  if (( cold_starts > 0 )); then
    for cold in $(seq 1 "$cold_starts"); do
      pair_id="$lane-cold-$cold"
      reset_pair_equivalence
      pair_lane="$lane"
      pair_sample_kind=cold
      if (( cold % 2 == 1 )); then
        run_lane_once stock "$lane" cold "$cold" "$pair_id" 1 "$stdin_payload" "${command[@]}"
        run_lane_once zmin "$lane" cold "$cold" "$pair_id" 2 "$stdin_payload" "${command[@]}"
      else
        run_lane_once zmin "$lane" cold "$cold" "$pair_id" 1 "$stdin_payload" "${command[@]}"
        run_lane_once stock "$lane" cold "$cold" "$pair_id" 2 "$stdin_payload" "${command[@]}"
      fi
      record_pair_equivalence
    done
  fi
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

"$python_bin" - "$summary_tsv" "$out_dir" "$artifact_identity" "$repo_root/tools" <<'PY'
import csv
import io
import math
import os
import pathlib
import statistics
import sys

path, output_root, output_identity, tools_dir = sys.argv[1:5]
sys.path.insert(0, tools_dir)
import performance_contract as contract

root = pathlib.Path(output_root)
rows = list(
    csv.DictReader(
        io.StringIO(
            contract.artifact_read_bytes(
                root,
                contract.artifact_relative_name(root, pathlib.Path(path)),
                expected_directory_identity=contract.parse_artifact_identity(output_identity),
            ).decode()
        ),
        delimiter="\t",
    )
)
rows = [row for row in rows if row.get("sample_kind") == "measured"]
lanes = sorted({row["lane"] for row in rows})
print(
    "lane\tstock_median\tzmin_median\ttime_median_ratio\t"
    "stock_p95\tzmin_p95\ttime_p95_ratio\t"
    "memory_metric\tmemory_semantics\tmemory_scope\tmemory_unit\t"
    "stock_memory_p95_bytes\tzmin_memory_p95_bytes\tmemory_p95_ratio"
)
failures = []
max_time_ratio = float(os.environ.get("ZMIN_OBSERVED_MAX_TIME_P95_RATIO", "0"))
max_memory_ratio = float(os.environ.get("ZMIN_OBSERVED_MAX_MEMORY_P95_RATIO", "0"))

def percentile(values, percentile):
    ordered = sorted(values)
    index = max(0, math.ceil(len(ordered) * percentile) - 1)
    return ordered[index]

for lane in lanes:
    stock = [float(row["real_seconds"]) for row in rows if row["lane"] == lane and row["tool"] == "stock"]
    zmin = [float(row["real_seconds"]) for row in rows if row["lane"] == lane and row["tool"] == "zmin"]
    identities = {
        tuple(row.get(field, "") for field in ("memory_metric", "memory_semantics", "memory_scope", "memory_unit"))
        for row in rows if row["lane"] == lane
    }
    if len(identities) != 1:
        failures.append(f"{lane}: inconsistent memory metric identity")
        continue
    memory_metric, memory_semantics, memory_scope, memory_unit = next(iter(identities))
    memory_field = "max_rss_bytes" if memory_metric == "peak_rss_bytes" else "job_commit_bytes"
    stock_memory = [
        int(row[memory_field])
        for row in rows
        if row["lane"] == lane and row["tool"] == "stock" and row[memory_field].isdigit()
    ]
    zmin_memory = [
        int(row[memory_field])
        for row in rows
        if row["lane"] == lane and row["tool"] == "zmin" and row[memory_field].isdigit()
    ]
    stock_median = statistics.median(stock)
    zmin_median = statistics.median(zmin)
    stock_p95 = percentile(stock, 0.95)
    zmin_p95 = percentile(zmin, 0.95)
    stock_memory_p95 = percentile(stock_memory, 0.95) if stock_memory else None
    zmin_memory_p95 = percentile(zmin_memory, 0.95) if zmin_memory else None
    median_ratio = zmin_median / stock_median if stock_median else float("inf")
    p95_ratio = zmin_p95 / stock_p95 if stock_p95 else float("inf")
    memory_ratio = zmin_memory_p95 / stock_memory_p95 if stock_memory_p95 and zmin_memory_p95 is not None else float("inf")
    memory_fields = (
        f"{memory_metric}\t{memory_semantics}\t{memory_scope}\t{memory_unit}\t"
        f"{stock_memory_p95}\t{zmin_memory_p95}\t{memory_ratio:.6f}"
        if stock_memory_p95 is not None and zmin_memory_p95 is not None
        else f"{memory_metric}\t{memory_semantics}\t{memory_scope}\t{memory_unit}\tunsupported\tunsupported\tunsupported"
    )
    print(
        f"{lane}\t{stock_median:.6f}\t{zmin_median:.6f}\t{median_ratio:.6f}\t"
        f"{stock_p95:.6f}\t{zmin_p95:.6f}\t{p95_ratio:.6f}\t{memory_fields}"
    )
    if max_time_ratio > 0 and p95_ratio > max_time_ratio:
        failures.append(f"{lane}: time p95 ratio {p95_ratio:.6f} > {max_time_ratio:.6f}")
    if max_memory_ratio > 0 and memory_ratio > max_memory_ratio:
        failures.append(f"{lane}: memory p95 ratio {memory_ratio:.6f} > {max_memory_ratio:.6f}")

if failures:
    raise SystemExit("observed client benchmark gate failed: " + "; ".join(failures))
PY

if [[ "$evidence_mode" == "authoritative" ]]; then
  artifact_cli superiority-summary \
    --metadata "$metadata_path" \
    --rows "$summary_tsv" \
    --output "$out_dir/superiority.tsv" \
    --results-dir "$out_dir" \
    --root-identity "$artifact_identity"
fi

if [[ "$evidence_mode" == "authoritative" ]]; then
  "$python_bin" "$repo_root/tools/performance_contract.py" finish \
    --metadata "$metadata_path" \
    --rows "$summary_tsv" \
    --output "$out_dir/evidence.json" \
    --results-dir "$out_dir" \
    --require-authoritative \
    "${finish_anchor_args[@]}"
else
  "$python_bin" "$repo_root/tools/performance_contract.py" finish \
    --metadata "$metadata_path" \
    --rows "$summary_tsv" \
    --output "$out_dir/evidence.json" \
    --results-dir "$out_dir" \
    "${finish_anchor_args[@]}"
fi

printf 'observed_client_bench_stock_git=%s\n' "$stock_git"
printf 'observed_client_bench_zmin_bin=%s\n' "$zmin_bin"
printf 'observed_client_bench_zmin_sha256=%s\n' \
  "$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$zmin_bin")"
printf 'observed_client_bench_phase_trace=%s\n' "$phase_trace"
printf 'observed_client_bench_warmups=%s\n' "$warmups"
printf 'observed_client_bench_cold_starts=%s\n' "$cold_starts"
printf 'observed_client_bench_claim=%s\n' "$(artifact_read_root "$out_dir" "$artifact_identity" evidence.json | \
  "$python_bin" -c 'import json,sys; print(json.load(sys.stdin)["claim_status"])')"
printf 'observed_client_bench_out=%s\n' "$out_dir"
