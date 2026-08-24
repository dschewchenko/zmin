#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$repo_root/tools/benchmark-environment.sh"
git_bin="${GIT_BIN:-}"
zmin_bin="${ZMIN_BIN:-$repo_root/target/release/zmin}"
gix_bin="${GIX_BIN:-$(command -v gix 2>/dev/null || true)}"
commits="${ZMIN_BENCH_COMMITS:-90}"
files_per_commit="${ZMIN_BENCH_FILES_PER_COMMIT:-25}"
clone_large_commits="${ZMIN_BENCH_CLONE_LARGE_COMMITS:-120}"
clone_large_files_per_commit="${ZMIN_BENCH_CLONE_LARGE_FILES_PER_COMMIT:-80}"
write_files="${ZMIN_BENCH_WRITE_FILES:-1800}"
dirty_files="${ZMIN_BENCH_DIRTY_FILES:-200}"
fetch_batch_files="${ZMIN_BENCH_FETCH_BATCH_FILES:-2400}"
push_batch_files="${ZMIN_BENCH_PUSH_BATCH_FILES:-2400}"
evidence_mode="${ZMIN_BENCH_EVIDENCE_MODE:-exploratory}"
if [[ "$evidence_mode" == "authoritative" && -z "$git_bin" ]]; then
  printf 'authoritative benchmark runs require an explicit pinned GIT_BIN\n' >&2
  exit 1
fi
if [[ -z "$git_bin" ]]; then
  git_bin="$(command -v git)"
fi
if [[ "$evidence_mode" == "authoritative" ]]; then
  warmups="${ZMIN_BENCH_WARMUPS:-3}"
  repeats="${ZMIN_BENCH_REPEATS:-30}"
  cold_starts="${ZMIN_BENCH_COLD_STARTS:-10}"
else
  warmups="${ZMIN_BENCH_WARMUPS:-0}"
  repeats="${ZMIN_BENCH_REPEATS:-10}"
  cold_starts="${ZMIN_BENCH_COLD_STARTS:-0}"
fi
gix_enabled=0
if [[ -x "$gix_bin" && "$evidence_mode" != "authoritative" ]]; then
  gix_enabled=1
fi
run_count=$((warmups + repeats + cold_starts))
seed="${ZMIN_BENCH_SEED:-1700000000}"
repack_max_pack_size="${ZMIN_BENCH_REPACK_MAX_PACK_SIZE:-}"
ops="${ZMIN_BENCH_OPS:-}"
out_dir="${ZMIN_BENCH_OUT_DIR:-}"
out_dir_explicit=0
if [[ -n "$out_dir" ]]; then
  out_dir_explicit=1
fi
phase_trace_dir="${ZMIN_BENCH_PHASE_TRACE_DIR:-}"
ssh_trace_dir="${ZMIN_BENCH_SSH_TRACE_DIR:-}"
ssh_packet_trace_dir="${ZMIN_BENCH_SSH_PACKET_TRACE_DIR:-}"
benchmark_authoritative_trace_preflight "$evidence_mode" \
  "$phase_trace_dir" "$ssh_trace_dir" "$ssh_packet_trace_dir"
python_bin="$(benchmark_resolve_python "$evidence_mode")"
if [[ "$evidence_mode" == "authoritative" && -z "${ZMIN_BIN:-}" ]]; then
  printf 'authoritative benchmark runs require an explicit prebuilt ZMIN_BIN release binary\n' >&2
  exit 1
fi
if [[ "$evidence_mode" == "authoritative" && -z "${ZMIN_BENCH_MAKE:-}" ]]; then
  printf 'authoritative benchmark requires explicit ZMIN_BENCH_MAKE\n' >&2
  exit 1
fi
make_bin="${ZMIN_BENCH_MAKE:-$(command -v make)}"
make_bin="$(cd "$(dirname "$make_bin")" && pwd -P)/$(basename "$make_bin")"
[[ -x "$make_bin" && ! -L "$make_bin" ]] || {
  printf 'benchmark Make binary is not a canonical executable: %s\n' "$make_bin" >&2
  exit 1
}

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

export GIT_CONFIG_NOSYSTEM="${GIT_CONFIG_NOSYSTEM:-1}"
export GIT_CONFIG_GLOBAL="${GIT_CONFIG_GLOBAL:-/dev/null}"

known_ops=(
  init
  status
  log
  rev-list
  merge-base
  pack-objects
  index-pack
  add
  commit
  add-dirty
  commit-dirty
  clone
  clone-large
  clone-instant
  clone-instant-git-daemon
  clone-instant-ssh
  push-noop
  push-incremental
  push-batch
  pull-noop
  pull-incremental
  fetch-noop
  fetch-incremental
  fetch-batch
)
standard_mandatory_ops=(
  init
  status
  log
  rev-list
  merge-base
  pack-objects
  index-pack
)

op_in_list() {
  local needle="$1"
  shift
  local candidate
  for candidate in "$@"; do
    if [[ "$candidate" == "$needle" ]]; then
      return 0
    fi
  done
  return 1
}

selected_ops=()
if [[ -n "${ops//[[:space:],;]/}" ]]; then
  while IFS= read -r op; do
    [[ -n "$op" ]] || continue
    if ! op_in_list "$op" "${known_ops[@]}"; then
      printf 'unknown benchmark operation %q. Known operations: %s\n' \
        "$op" "${known_ops[*]}" >&2
      exit 1
    fi
    if [[ "$evidence_mode" == "authoritative" && "${#selected_ops[@]}" -gt 0 ]] \
      && op_in_list "$op" "${selected_ops[@]}"; then
      printf 'authoritative benchmark operation list contains a duplicate lane: %s\n' "$op" >&2
      exit 1
    fi
    if [[ "${#selected_ops[@]}" -eq 0 ]] || ! op_in_list "$op" "${selected_ops[@]}"; then
      selected_ops+=("$op")
    fi
  done < <(printf '%s\n' "$ops" | tr ',;' '\n\n' | tr '[:space:]' '\n')
fi

if [[ "$evidence_mode" == "authoritative" ]]; then
  if [[ "${#selected_ops[@]}" -eq 0 ]]; then
    selected_ops=("${standard_mandatory_ops[@]}")
  fi
  if [[ "${#selected_ops[@]}" -ne "${#standard_mandatory_ops[@]}" ]]; then
    printf 'authoritative standard scope requires exactly these lanes in order: %s\n' \
      "${standard_mandatory_ops[*]}" >&2
    exit 1
  fi
  for index in "${!standard_mandatory_ops[@]}"; do
    if [[ "${selected_ops[$index]}" != "${standard_mandatory_ops[$index]}" ]]; then
      printf 'authoritative standard scope requires exact lane order: %s\n' \
        "${standard_mandatory_ops[*]}" >&2
      exit 1
    fi
  done
fi

selected_ops_label() {
  local selected=()
  local op
  for op in "${known_ops[@]}"; do
    if [[ "${#selected_ops[@]}" -gt 0 ]] && op_in_list "$op" "${selected_ops[@]}"; then
      selected+=("$op")
    fi
  done
  (IFS=,; printf '%s' "${selected[*]}")
}

benchmark_op_enabled() {
  local op="$1"
  [[ "${#selected_ops[@]}" -eq 0 ]] || op_in_list "$op" "${selected_ops[@]}"
}

any_benchmark_op_enabled() {
  local op
  for op in "$@"; do
    if benchmark_op_enabled "$op"; then
      return 0
    fi
  done
  return 1
}

shell_quote() {
  printf '%q' "$1"
}

if [[ "${ZMIN_BENCH_QUOTE_TEST:-0}" == "1" ]]; then
  quoted_path="$(shell_quote "${ZMIN_BENCH_QUOTE_PATH:?ZMIN_BENCH_QUOTE_PATH is required}")"
  round_trip="$(bash -c "printf '%s' $quoted_path")"
  [[ "$round_trip" == "$ZMIN_BENCH_QUOTE_PATH" ]] || {
    printf 'shell quote regression: %s != %s\n' "$round_trip" "$ZMIN_BENCH_QUOTE_PATH" >&2
    exit 1
  }
  exit 0
fi

if [[ "${#selected_ops[@]}" -gt 0 ]]; then
  printf 'selected_ops=%s\n' "$(selected_ops_label)" >&2
fi

if [[ "$evidence_mode" == "authoritative" && -z "${ZMIN_BIN:-}" ]]; then
  printf 'authoritative benchmark runs require an explicit prebuilt ZMIN_BIN release binary\n' >&2
  exit 1
fi
if [[ ! -x "$zmin_bin" ]]; then
  printf 'zmin release binary was not found or is not executable: %s\n' "$zmin_bin" >&2
  exit 1
fi
zmin_bin="$(cd "$(dirname "$zmin_bin")" && pwd)/$(basename "$zmin_bin")"
git_bin="$(cd "$(dirname "$git_bin")" && pwd)/$(basename "$git_bin")"
comparator_bundle=""
if [[ "$evidence_mode" == "authoritative" ]]; then
  comparator_bundle="$(benchmark_validate_authoritative_git_comparator "$repo_root" "$git_bin")" || exit 1
fi

if [[ -n "$phase_trace_dir" ]]; then
  mkdir -p "$phase_trace_dir"
fi

if [[ -n "$ssh_trace_dir" ]]; then
  mkdir -p "$ssh_trace_dir"
fi

if [[ -n "$ssh_packet_trace_dir" ]]; then
  mkdir -p "$ssh_packet_trace_dir"
fi

tmp_dir="$(mktemp -d /tmp/zmin-performance-bench.XXXXXX)"
tmp_artifact_identity="$(artifact_preflight_root "$tmp_dir" --name bench.tsv --name validation.tsv)"
daemon_pid=""
cleanup() {
  if [[ -n "${daemon_pid:-}" ]]; then
    kill "$daemon_pid" >/dev/null 2>&1 || true
    wait "$daemon_pid" >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

benchmark_sanitize_environment "$tmp_dir" "$git_bin" "$zmin_bin" "$python_bin"
if [[ "$evidence_mode" == "authoritative" ]]; then
  export ZMIN_BENCH_GIT_COMPARATOR_STATUS=validated
  export ZMIN_BENCH_GIT_COMPARATOR_BUNDLE="$comparator_bundle"
  export ZMIN_BENCH_GIT_COMPARATOR_CONTRACT='v2.55.0;e9019fcafe0040228b8631c30f97ae1adb61bcdc;72923418db7b26dfddc21e2268660c5118e560bdfaa09b4489b67b38e9b69c49'
fi
if [[ "$evidence_mode" == "authoritative" && ( "$out_dir_explicit" != "1" || "$out_dir" != /* ) ]]; then
  printf 'authoritative benchmark runs require an explicit absolute ZMIN_BENCH_OUT_DIR\n' >&2
  exit 1
fi
if [[ -z "$out_dir" ]]; then
  out_dir="$(mktemp -d /tmp/zmin-performance-bench-evidence.XXXXXX)"
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
evidence_artifact_identity="$(artifact_preflight_root "$out_dir" \
  --name metadata.json --name evidence.json --name bench.tsv --name checks.tsv \
  --name equivalence.tsv \
  --name summary.csv --name comparison.csv --name superiority.tsv)"

out="$tmp_dir/bench.tsv"
validation_out="$tmp_dir/validation.tsv"
src="$tmp_dir/src"
remote="$tmp_dir/remote.git"
artifact_write_root "$tmp_dir" "$tmp_artifact_identity" bench.tsv \
  $'tool\top\tsample_kind\tpair_id\torder_index\treal\tuser\tsys\trss_bytes\tjob_commit_bytes\tmajor_page_faults\tminor_page_faults\tread_bytes\twrite_bytes\tmemory_metric\tmemory_semantics\tmemory_scope\tmemory_unit\tmetrics_availability\texit\textra\n'
artifact_write_root "$tmp_dir" "$tmp_artifact_identity" validation.tsv $'check\tstatus\tdetails\n'
artifact_write_root "$out_dir" "$evidence_artifact_identity" equivalence.tsv \
  $'lane\tsample_kind\tphase\tpair_id\tpair_index\tgit_order_index\tzmin_order_index\tgit_exit\tzmin_exit\tgit_stdout_sha256\tzmin_stdout_sha256\tgit_stderr_sha256\tzmin_stderr_sha256\texit_equal\tstdout_equal\tstderr_equal\n'

record_validation() {
  local validation_row="$1"$'\t'"$2"$'\t'"$3"
  validation_row+=$'\n'
  artifact_append_root "$tmp_dir" "$tmp_artifact_identity" validation.tsv \
    "$validation_row"
}

phase_trace_file_for() {
  local tool="$1" op="$2" extra="$3"
  local safe_extra
  safe_extra="$(printf '%s' "$extra" | tr -c '[:alnum:]._+-' '-')"
  printf '%s/%s-%s-%s-%s.log' \
    "$phase_trace_dir" \
    "$op" \
    "$tool" \
    "$safe_extra" \
    "$(date +%s%N)"
}

ssh_trace_file_for() {
  local tool="$1" op="$2" extra="$3"
  local safe_extra
  safe_extra="$(printf '%s' "$extra" | tr -c '[:alnum:]._+-' '-')"
  printf '%s/%s-%s-%s-%s.tsv' \
    "$ssh_trace_dir" \
    "$op" \
    "$tool" \
    "$safe_extra" \
    "$(date +%s%N)"
}

ssh_packet_trace_file_for() {
  local tool="$1" op="$2" extra="$3"
  local safe_extra
  safe_extra="$(printf '%s' "$extra" | tr -c '[:alnum:]._+-' '-')"
  printf '%s/%s-%s-%s-%s.packet.log' \
    "$ssh_packet_trace_dir" \
    "$op" \
    "$tool" \
    "$safe_extra" \
    "$(date +%s%N)"
}

reset_pair_equivalence() {
  pair_git_exit=""
  pair_git_order=""
  pair_git_stdout_sha256=""
  pair_git_stderr_sha256=""
  pair_zmin_exit=""
  pair_zmin_order=""
  pair_zmin_stdout_sha256=""
  pair_zmin_stderr_sha256=""
  pair_git_stdout_file=""
  pair_git_stderr_file=""
  pair_git_metrics_file=""
  pair_zmin_stdout_file=""
  pair_zmin_stderr_file=""
  pair_zmin_metrics_file=""
}

retain_pair_artifacts() {
  local source
  for source in \
    "$pair_git_stdout_file" "$pair_git_stderr_file" "$pair_git_metrics_file" \
    "$pair_zmin_stdout_file" "$pair_zmin_stderr_file" "$pair_zmin_metrics_file"; do
    if [[ -f "$source" ]]; then
      artifact_copy_root "$out_dir" "$evidence_artifact_identity" \
        "$(basename "$source")" "$source"
    fi
  done
}

record_pair_equivalence() {
  if [[ -z "$pair_git_exit" || -z "$pair_zmin_exit" ]]; then
    return
  fi
  local exit_equal=false stdout_equal=false stderr_equal=false
  [[ "$pair_git_exit" == "$pair_zmin_exit" ]] && exit_equal=true
  [[ "$pair_git_stdout_sha256" == "$pair_zmin_stdout_sha256" ]] && stdout_equal=true
  [[ "$pair_git_stderr_sha256" == "$pair_zmin_stderr_sha256" ]] && stderr_equal=true
  if [[ "$exit_equal" != true || "$stdout_equal" != true || "$stderr_equal" != true ]]; then
    retain_pair_artifacts
    printf 'Git/Zmin output equivalence mismatch for %s\n' "$pair_id" >&2
    exit 1
  fi
  local equivalence_row
  local phase="$sample_kind"
  [[ "$sample_kind" == cold ]] && phase=process-cold
  equivalence_row="$op"$'\t'"$sample_kind"$'\t'"$phase"$'\t'"$pair_id"$'\t'"${pair_id##*-}"$'\t'"$pair_git_order"$'\t'"$pair_zmin_order"$'\t'"$pair_git_exit"$'\t'"$pair_zmin_exit"$'\t'"$pair_git_stdout_sha256"$'\t'"$pair_zmin_stdout_sha256"$'\t'"$pair_git_stderr_sha256"$'\t'"$pair_zmin_stderr_sha256"$'\t'"$exit_equal"$'\t'"$stdout_equal"$'\t'"$stderr_equal"$'\n'
  artifact_append_root "$out_dir" "$evidence_artifact_identity" equivalence.tsv \
    "$equivalence_row"
}

measure_sh() {
  local tool="$1" op="$2" extra="$3" script="$4" sample_kind="$5" pair_id="$6" order_index="$7"
  if [[ "$op" == "init" ]]; then
    artifact_cli template-preflight \
      --fixture-root "$src" \
      --template-dir "$init_template_dir" \
      --expected-identity "$init_template_identity" >/dev/null
  fi
  local metric_file="$tmp_dir/metrics-$tool-$op-$(date +%s%N).tsv"
  local stdout_file="$tmp_dir/stdout-$tool-$op-$(date +%s%N).txt"
  local stderr_file="$tmp_dir/stderr-$tool-$op-$(date +%s%N).txt"
  local trace_env=()
  if [[ "$tool" == "zmin" && -n "$phase_trace_dir" ]]; then
    trace_file="$(phase_trace_file_for "$tool" "$op" "$extra")"
    trace_env=(
      "ZMIN_PHASE_TRACE=1"
      "ZMIN_CHECKOUT_PHASE_TRACE=1"
      "ZMIN_PHASE_TRACE_FILE=$trace_file"
    )
  fi
  if [[ "$op" == "clone-instant-ssh" && -n "$ssh_trace_dir" ]]; then
    trace_env+=(
      "ZMIN_BENCH_SSH_TRACE_FILE=$(ssh_trace_file_for "$tool" "$op" "$extra")"
      "ZMIN_BENCH_SSH_TRACE_TOOL=$tool"
      "ZMIN_BENCH_SSH_TRACE_OP=$op"
      "ZMIN_BENCH_SSH_TRACE_EXTRA=$extra"
    )
  fi
  if [[ "$op" == "clone-instant-ssh" && -n "$ssh_packet_trace_dir" ]]; then
    trace_env+=(
      "GIT_TRACE_PACKET=$(ssh_packet_trace_file_for "$tool" "$op" "$extra")"
    )
  fi
  set +e
  if [[ "$op" == "init" ]]; then
    if [[ "${#trace_env[@]}" -gt 0 ]]; then
      env "${trace_env[@]}" "$python_bin" "$repo_root/tools/git-bench-process.py" \
        --artifact-root "$tmp_dir" --artifact-root-identity "$tmp_artifact_identity" \
        --stdout "$stdout_file" --stderr "$stderr_file" --metrics "$metric_file" \
        --bound-directory-env GIT_TEMPLATE_DIR \
        --bound-directory "$init_template_dir" \
        --bound-directory-identity "$init_template_identity" \
        -- bash -c "$script" --
    else
      "$python_bin" "$repo_root/tools/git-bench-process.py" \
        --artifact-root "$tmp_dir" --artifact-root-identity "$tmp_artifact_identity" \
        --stdout "$stdout_file" --stderr "$stderr_file" --metrics "$metric_file" \
        --bound-directory-env GIT_TEMPLATE_DIR \
        --bound-directory "$init_template_dir" \
        --bound-directory-identity "$init_template_identity" \
        -- bash -c "$script" --
    fi
  else
    if [[ "${#trace_env[@]}" -gt 0 ]]; then
      env "${trace_env[@]}" "$python_bin" "$repo_root/tools/git-bench-process.py" \
        --artifact-root "$tmp_dir" --artifact-root-identity "$tmp_artifact_identity" \
        --stdout "$stdout_file" --stderr "$stderr_file" --metrics "$metric_file" \
        -- bash -c "$script" --
    else
      "$python_bin" "$repo_root/tools/git-bench-process.py" \
        --artifact-root "$tmp_dir" --artifact-root-identity "$tmp_artifact_identity" \
        --stdout "$stdout_file" --stderr "$stderr_file" --metrics "$metric_file" \
        -- bash -c "$script" --
    fi
  fi
  local status=$?
  set -e
  local real_seconds user_seconds sys_seconds max_rss_bytes job_commit_bytes major_faults minor_faults read_bytes write_bytes memory_metric memory_semantics memory_scope memory_unit metrics_availability
  local metrics
  metrics="$(artifact_read_root "$tmp_dir" "$tmp_artifact_identity" "$(basename "$metric_file")")"
  IFS=$'\t' read -r real_seconds user_seconds sys_seconds max_rss_bytes job_commit_bytes major_faults minor_faults read_bytes write_bytes memory_metric memory_semantics memory_scope memory_unit metrics_availability <<<"$metrics"
  local row="$tool"$'\t'"$op"$'\t'"$sample_kind"$'\t'"$pair_id"$'\t'"$order_index"$'\t'"$real_seconds"$'\t'"$user_seconds"$'\t'"$sys_seconds"$'\t'"$max_rss_bytes"$'\t'"$job_commit_bytes"$'\t'"$major_faults"$'\t'"$minor_faults"$'\t'"$read_bytes"$'\t'"$write_bytes"$'\t'"$memory_metric"$'\t'"$memory_semantics"$'\t'"$memory_scope"$'\t'"$memory_unit"$'\t'"$metrics_availability"$'\t'"$status"$'\t'"$extra"
  row+=$'\n'
  artifact_append_root "$tmp_dir" "$tmp_artifact_identity" bench.tsv \
    "$row"
  local stdout_sha256 stderr_sha256
  stdout_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$stdout_file")"
  stderr_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$stderr_file")"
  if [[ "$tool" == "git" ]]; then
    pair_git_exit="$status"
    pair_git_order="$order_index"
    pair_git_stdout_sha256="$stdout_sha256"
    pair_git_stderr_sha256="$stderr_sha256"
    pair_git_stdout_file="$stdout_file"
    pair_git_stderr_file="$stderr_file"
    pair_git_metrics_file="$metric_file"
  elif [[ "$tool" == "zmin" ]]; then
    pair_zmin_exit="$status"
    pair_zmin_order="$order_index"
    pair_zmin_stdout_sha256="$stdout_sha256"
    pair_zmin_stderr_sha256="$stderr_sha256"
    pair_zmin_stdout_file="$stdout_file"
    pair_zmin_stderr_file="$stderr_file"
    pair_zmin_metrics_file="$metric_file"
  fi
  record_pair_equivalence
}

set_sample_policy() {
  local sample_index="$1"
  if (( sample_index <= warmups )); then
    BENCH_SAMPLE_KIND=warmup
    BENCH_SAMPLE_ID="warmup-$sample_index"
  elif (( sample_index <= warmups + repeats )); then
    BENCH_SAMPLE_KIND=measured
    BENCH_SAMPLE_ID="measured-$((sample_index - warmups))"
  else
    BENCH_SAMPLE_KIND=cold
    BENCH_SAMPLE_ID="cold-$((sample_index - warmups - repeats))"
  fi
  export BENCH_SAMPLE_KIND BENCH_SAMPLE_ID
}

run_group() {
  local op="$1" extra="$2" group_seed="$3"
  shift 3
  local spec_file="$tmp_dir/spec-$op-$group_seed.tsv"
  printf '%s\n' "$@" >"$spec_file"
  reset_pair_equivalence
  local order_index=0
  local sample_kind="${BENCH_SAMPLE_KIND:-measured}"
  local pair_id="$op-${BENCH_SAMPLE_ID:-measured-1}"
  while IFS=$'\t' read -r tool script; do
    order_index=$((order_index + 1))
    measure_sh "$tool" "$op" "$extra" "$script" "$sample_kind" "$pair_id" "$order_index"
  done < <("$python_bin" - "$group_seed" "$spec_file" <<'PY'
import random
import sys

seed = int(sys.argv[1])
path = sys.argv[2]
with open(path, encoding="utf-8") as handle:
    items = [line.rstrip("\n") for line in handle if line.rstrip("\n")]
rng = random.Random(seed)
rng.shuffle(items)
for item in items:
    print(item)
PY
  )
}

make_files() {
  local dir="$1" count="$2" prefix="${3:-file}"
  mkdir -p "$dir"
  for i in $(seq 1 "$count"); do
    mkdir -p "$dir/dir-$((i % 32))"
    printf '%s=%05d\npayload=%04096d\n' "$prefix" "$i" 0 >"$dir/dir-$((i % 32))/file-$i.txt"
  done
}

compare_files() {
  local name="$1" left="$2" right="$3"
  if cmp -s "$left" "$right"; then
    record_validation "$name" ok "matched"
  else
    record_validation "$name" fail "mismatch"
    diff -u "$left" "$right" >&2 || true
    exit 1
  fi
}

compare_refs() {
  local name="$1" left_repo="$2" right_repo="$3" ref="$4"
  local left right
  left="$("$git_bin" -C "$left_repo" rev-parse "$ref")"
  right="$("$git_bin" -C "$right_repo" rev-parse "$ref")"
  if [[ "$left" == "$right" ]]; then
    record_validation "$name" ok "$ref=$left"
  else
    record_validation "$name" fail "$ref: $left != $right"
    exit 1
  fi
}

compare_trees() {
  local name="$1" left_repo="$2" right_repo="$3" ref="${4:-HEAD}"
  local left right
  left="$("$git_bin" -C "$left_repo" rev-parse "$ref^{tree}")"
  right="$("$git_bin" -C "$right_repo" rev-parse "$ref^{tree}")"
  if [[ "$left" == "$right" ]]; then
    record_validation "$name" ok "$ref tree=$left"
  else
    record_validation "$name" fail "$ref tree: $left != $right"
    exit 1
  fi
}

check_worktree_first_marker() {
  local name="$1" repo="$2"
  if [[ "$("$git_bin" -C "$repo" config --get zmin.worktreeFirst)" == "true" ]]; then
    record_validation "$name" ok "zmin.worktreeFirst=true"
  else
    record_validation "$name" fail "missing zmin.worktreeFirst=true"
    exit 1
  fi
}

unused_local_port() {
  "$python_bin" - <<'PY'
import socket

with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
}

start_git_daemon() {
  local base_path="$1" url="$2" port="$3"
  "$git_bin" daemon \
    --reuseaddr \
    --base-path="$base_path" \
    --export-all \
    --listen=127.0.0.1 \
    --port="$port" \
    "$base_path" >"$tmp_dir/git-daemon.stdout" 2>"$tmp_dir/git-daemon.stderr" &
  daemon_pid=$!
  for _ in $(seq 1 100); do
    if "$git_bin" ls-remote "$url" HEAD >/dev/null 2>&1; then
      return
    fi
    sleep 0.1
  done
  cat "$tmp_dir/git-daemon.stderr" >&2 || true
  echo "git daemon did not become ready" >&2
  exit 1
}

write_fake_ssh() {
  local script="$tmp_dir/fake-ssh.sh"
  cat >"$script" <<'SH'
#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in
    -p|-l|-o|-F|-i|-J)
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -*)
      shift
      ;;
    *)
      break
      ;;
  esac
done
if [ "$#" -lt 2 ]; then
  echo "fake ssh missing remote command" >&2
  exit 1
fi
shift
cmd="$*"
cmd="$(printf '%s\n' "$cmd" | sed -E "s#'/(.):#'\1:#g; s#\"/(.):#\"\1:#g; s# /(.:)# \1#g")"
if [ "${ZMIN_BENCH_FAKE_SSH_GIT_EXEC_PATH:-}" ]; then
  PATH="$ZMIN_BENCH_FAKE_SSH_GIT_EXEC_PATH:$PATH"
  export PATH
fi
if [ "${ZMIN_BENCH_SSH_TRACE_FILE:-}" ]; then
  trace_file="$ZMIN_BENCH_SSH_TRACE_FILE"
  if [ ! -s "$trace_file" ]; then
    printf 'tool\top\textra\tgit_protocol\tstart_ns\tend_ns\treal_seconds\texit\tcommand\n' >"$trace_file"
  fi
  start_ns="$(date +%s%N 2>/dev/null || date +%s000000000)"
  set +e
  /bin/sh -c "$cmd"
  status=$?
  set -e
  end_ns="$(date +%s%N 2>/dev/null || date +%s000000000)"
  real_seconds="$(awk -v start="$start_ns" -v end="$end_ns" 'BEGIN { printf "%.6f", (end - start) / 1000000000 }')"
  safe_cmd="$(printf '%s' "$cmd" | tr '\t\r\n' '   ')"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${ZMIN_BENCH_SSH_TRACE_TOOL:-}" \
    "${ZMIN_BENCH_SSH_TRACE_OP:-}" \
    "${ZMIN_BENCH_SSH_TRACE_EXTRA:-}" \
    "${GIT_PROTOCOL:-}" \
    "$start_ns" \
    "$end_ns" \
    "$real_seconds" \
    "$status" \
    "$safe_cmd" >>"$trace_file"
  exit "$status"
fi
exec /bin/sh -c "$cmd"
SH
  chmod +x "$script"
  printf '%s\n' "$script"
}

validate_clean_git_zmin_outputs() {
  "$git_bin" -C "$src" status --porcelain=v1 --branch >"$tmp_dir/git-status.txt"
  "$zmin_bin" -C "$src" status --porcelain=v1 --branch >"$tmp_dir/zmin-status.txt"
  compare_files status "$tmp_dir/git-status.txt" "$tmp_dir/zmin-status.txt"

  "$git_bin" -C "$src" log --oneline --max-count "$commits" >"$tmp_dir/git-log.txt"
  "$zmin_bin" -C "$src" log --oneline --max-count "$commits" >"$tmp_dir/zmin-log.txt"
  compare_files log "$tmp_dir/git-log.txt" "$tmp_dir/zmin-log.txt"

  "$git_bin" -C "$src" rev-list --objects --all >"$tmp_dir/git-rev-list.txt"
  "$zmin_bin" -C "$src" rev-list --objects --all >"$tmp_dir/zmin-rev-list.txt"
  compare_files rev-list "$tmp_dir/git-rev-list.txt" "$tmp_dir/zmin-rev-list.txt"

  "$git_bin" -C "$src" merge-base HEAD "HEAD~$((commits / 2))" >"$tmp_dir/git-merge-base.txt"
  "$zmin_bin" -C "$src" merge-base HEAD "HEAD~$((commits / 2))" >"$tmp_dir/zmin-merge-base.txt"
  compare_files merge-base "$tmp_dir/git-merge-base.txt" "$tmp_dir/zmin-merge-base.txt"
}

validate_pack_output() {
  "$git_bin" -C "$src" pack-objects --stdout <"$tmp_dir/objects.txt" >"$tmp_dir/validate-git.pack"
  "$zmin_bin" -C "$src" pack-objects --stdout <"$tmp_dir/objects.txt" >"$tmp_dir/validate-zmin.pack"
  "$git_bin" init -q "$tmp_dir/validate-git-index"
  "$git_bin" init -q "$tmp_dir/validate-zmin-index"
  "$git_bin" -C "$tmp_dir/validate-git-index" index-pack --stdin <"$tmp_dir/validate-git.pack" >/dev/null
  "$git_bin" -C "$tmp_dir/validate-zmin-index" index-pack --stdin <"$tmp_dir/validate-zmin.pack" >/dev/null
  record_validation pack-objects ok "git_index_pack_accepts_git_and_zmin_packs"
}

configure_repo() {
  local repo="$1"
  "$git_bin" -C "$repo" config user.name Bench
  "$git_bin" -C "$repo" config user.email bench@example.test
  "$git_bin" -C "$repo" config commit.gpgsign false
}

create_source_repo() {
  local repo="$1" commit_count="$2" files_per_commit_count="$3"

  "$git_bin" init -q -b main "$repo"
  configure_repo "$repo"

  for c in $(seq 1 "$commit_count"); do
    mkdir -p "$repo/dir-$((c % 24))"
    for f in $(seq 1 "$files_per_commit_count"); do
      printf 'commit=%03d file=%03d payload=%04096d\n' "$c" "$f" 0 \
        >"$repo/dir-$((c % 24))/file-$f.txt"
    done
    "$git_bin" -C "$repo" add -A
    ts=$((1700000000 + c))
    GIT_AUTHOR_DATE="$ts +0000" GIT_COMMITTER_DATE="$ts +0000" \
      "$git_bin" -C "$repo" commit -qm "commit $c"
  done

  if [[ -n "$repack_max_pack_size" ]]; then
    "$git_bin" -C "$repo" repack -adq --max-pack-size="$repack_max_pack_size"
  else
    "$git_bin" -C "$repo" repack -adq
  fi
  "$git_bin" -C "$repo" fsck --strict >/dev/null
}

create_source_repo "$src" "$commits" "$files_per_commit"
"$git_bin" -C "$src" rev-list --objects --all --no-object-names >"$tmp_dir/objects.txt"
object_count="$(wc -l <"$tmp_dir/objects.txt" | tr -d ' ')"
if any_benchmark_op_enabled status log rev-list merge-base; then
  validate_clean_git_zmin_outputs
fi
if any_benchmark_op_enabled pack-objects index-pack; then
  validate_pack_output
fi

prepare_fetch_fixtures() {
  if any_benchmark_op_enabled fetch-noop fetch-incremental; then
    "$git_bin" init -q --bare "$remote"
    "$git_bin" -C "$src" remote add origin "$remote"
    "$git_bin" -C "$src" push -q origin main
    "$git_bin" clone -q "$remote" "$tmp_dir/git-fetch"
    "$zmin_bin" clone -q "$remote" "$tmp_dir/zmin-fetch" >/dev/null
    if [[ "$gix_enabled" == "1" ]]; then
      "$git_bin" clone -q "$remote" "$tmp_dir/gix-fetch"
    fi

    if benchmark_op_enabled fetch-incremental; then
      fetch_incremental_src="$tmp_dir/fetch-incremental-source"
      fetch_incremental_remote="$tmp_dir/fetch-incremental-remote.git"
      "$git_bin" clone -q "$src" "$fetch_incremental_src"
      "$git_bin" -C "$fetch_incremental_src" remote remove origin
      "$git_bin" init -q --bare "$fetch_incremental_remote"
      "$git_bin" -C "$fetch_incremental_src" remote add origin "$fetch_incremental_remote"
      "$git_bin" -C "$fetch_incremental_src" push -q origin main
      printf 'new\n' >"$fetch_incremental_src/new-file.txt"
      "$git_bin" -C "$fetch_incremental_src" add new-file.txt
      GIT_AUTHOR_DATE='1700099999 +0000' GIT_COMMITTER_DATE='1700099999 +0000' \
        "$git_bin" -C "$fetch_incremental_src" commit -qm new
      "$git_bin" -C "$fetch_incremental_src" push -q origin main
    fi
  fi

  if benchmark_op_enabled fetch-batch; then
    batch_src="$tmp_dir/batch-src"
    batch_remote="$tmp_dir/batch-remote.git"
    "$git_bin" init -q -b main "$batch_src"
    configure_repo "$batch_src"
    mkdir -p "$batch_src/base"
    for i in $(seq 1 300); do
      printf 'base %04d %04096d\n' "$i" 0 >"$batch_src/base/file-$i.txt"
    done
    "$git_bin" -C "$batch_src" add -A
    GIT_AUTHOR_DATE='1700100000 +0000' GIT_COMMITTER_DATE='1700100000 +0000' \
      "$git_bin" -C "$batch_src" commit -qm base
    "$git_bin" init -q --bare "$batch_remote"
    "$git_bin" -C "$batch_src" remote add origin "$batch_remote"
    "$git_bin" -C "$batch_src" push -q origin main
    "$git_bin" clone -q "$batch_remote" "$tmp_dir/git-fetch-batch-base"
    "$zmin_bin" clone -q "$batch_remote" "$tmp_dir/zmin-fetch-batch-base" >/dev/null
    if [[ "$gix_enabled" == "1" ]]; then
      "$git_bin" clone -q "$batch_remote" "$tmp_dir/gix-fetch-batch-base"
    fi
    mkdir -p "$batch_src/batch"
    for i in $(seq 1 "$fetch_batch_files"); do
      printf 'batch %04d %04096d\n' "$i" 0 >"$batch_src/batch/file-$i.txt"
    done
    "$git_bin" -C "$batch_src" add -A
    GIT_AUTHOR_DATE='1700100001 +0000' GIT_COMMITTER_DATE='1700100001 +0000' \
      "$git_bin" -C "$batch_src" commit -qm batch
    "$git_bin" -C "$batch_src" push -q origin main
  fi
}

prepare_fetch_fixtures

init_template_dir="$src/.zmin-bench-empty-template"
mkdir "$init_template_dir"
artifact_cli template-preflight \
  --fixture-root "$src" \
  --template-dir "$init_template_dir" >/dev/null
init_template_dir="$(
  cd -- "$init_template_dir"
  pwd -P
)"
init_template_identity="$(
  artifact_cli template-preflight \
    --fixture-root "$src" \
    --template-dir "$init_template_dir"
)"
export GIT_TEMPLATE_DIR="$init_template_dir"

evidence_dir="${out_dir:-$tmp_dir/evidence}"
metadata_path="$evidence_dir/metadata.json"
mandatory_manifest="pilot"
if [[ "$evidence_mode" == "authoritative" ]]; then
  mandatory_manifest="standard"
fi
command_corpus="git-performance-bench-v3:${mandatory_manifest}:$(selected_ops_label)"
if [[ "$evidence_mode" == "authoritative" ]]; then
  fixture_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" fixture-hash --root "$src" --git-bin "$git_bin" --template-dir "$init_template_dir" --template-identity "$init_template_identity" --strict-paths)"
else
  fixture_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" fixture-hash --root "$src" --git-bin "$git_bin" --template-dir "$init_template_dir" --template-identity "$init_template_identity")"
fi
git_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$git_bin")"
zmin_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$zmin_bin")"
git_version="$($git_bin --version)"
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
  "$repo_root/tools/git-performance-bench.sh"
  "$repo_root/tools/git-bench-process.py"
  "$repo_root/tools/benchmark-environment.sh"
  "$repo_root/tools/performance_contract.py"
)
start_harness_args=()
finish_anchor_args=(
  --anchor-repo-root "$repo_root"
  --anchor-fixture-root "$src"
  --anchor-command-corpus "$command_corpus"
  --anchor-fixture-sha256 "$fixture_sha256"
  --anchor-git-bin "$git_bin"
  --anchor-git-sha256 "$git_sha256"
  --anchor-git-version "$git_version"
  --anchor-zmin-bin "$zmin_bin"
  --anchor-zmin-sha256 "$zmin_sha256"
  --anchor-zmin-version "$zmin_version"
  --anchor-python-bin "$python_bin"
  --anchor-python-sha256 "$python_sha256"
  --anchor-python-version "$python_version"
  --anchor-make-bin "$make_bin"
  --anchor-make-sha256 "$make_sha256"
  --anchor-make-version "$make_version"
  --anchor-build-profile release
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
  --git-bin "$git_bin" \
  --zmin-bin "$zmin_bin" \
  --python-bin "$python_bin" \
  --make-bin "$make_bin" \
  --fixture-root "$src" \
  --results-dir "$evidence_dir" \
  --output "$metadata_path" \
  --mode "$evidence_mode" \
  --build-profile release \
  --command-corpus "$command_corpus" \
  --mandatory-manifest "$mandatory_manifest" \
  --mandatory-lanes "$(selected_ops_label)" \
  --equivalence-manifest equivalence.tsv \
  --warmups "$warmups" \
  --measured-pairs "$repeats" \
  --cold-starts "$cold_starts" \
  --ordering interleaved-paired \
  --seed "$seed" \
  "${start_harness_args[@]}"
start_metadata_sha256="$("$python_bin" "$repo_root/tools/performance_contract.py" hash-file --path "$metadata_path")"
finish_anchor_args+=(--anchor-start-metadata-sha256 "$start_metadata_sha256")

for n in $(seq 1 "$run_count"); do
  set_sample_policy "$n"
  if benchmark_op_enabled init; then
    specs=(
      $'git\t'"$(shell_quote "$git_bin") init -q $(shell_quote "$tmp_dir/git-init-$n")"
      $'zmin\t'"$(shell_quote "$zmin_bin") init -q $(shell_quote "$tmp_dir/zmin-init-$n")"
    )
    run_group init "$n" "$((seed + n))" "${specs[@]}"
  fi

  if benchmark_op_enabled status; then
    specs=(
      $'git\t'"cd $(shell_quote "$src") && $(shell_quote "$git_bin") status --porcelain=v1 --branch"
      $'zmin\t'"cd $(shell_quote "$src") && $(shell_quote "$zmin_bin") status --porcelain=v1 --branch"
    )
    if [[ "$gix_enabled" == "1" ]]; then
      specs+=($'gix\t'"$(shell_quote "$gix_bin") -r $(shell_quote "$src") status --format simplified")
    fi
    run_group status "$n" "$((seed + 100 + n))" "${specs[@]}"
  fi

  if benchmark_op_enabled log; then
    specs=(
      $'git\t'"cd $(shell_quote "$src") && $(shell_quote "$git_bin") log --oneline --max-count $commits"
      $'zmin\t'"cd $(shell_quote "$src") && $(shell_quote "$zmin_bin") log --oneline --max-count $commits"
    )
    if [[ "$gix_enabled" == "1" ]]; then
      specs+=($'gix\t'"$(shell_quote "$gix_bin") -r $(shell_quote "$src") log")
    fi
    run_group log "$n" "$((seed + 200 + n))" "${specs[@]}"
  fi

  if benchmark_op_enabled rev-list; then
    run_group rev-list "$n" "$((seed + 300 + n))" \
      $'git\t'"cd $(shell_quote "$src") && $(shell_quote "$git_bin") rev-list --objects --all" \
      $'zmin\t'"cd $(shell_quote "$src") && $(shell_quote "$zmin_bin") rev-list --objects --all"
  fi

  if benchmark_op_enabled merge-base; then
    specs=(
      $'git\t'"cd $(shell_quote "$src") && $(shell_quote "$git_bin") merge-base HEAD HEAD~$((commits / 2))"
      $'zmin\t'"cd $(shell_quote "$src") && $(shell_quote "$zmin_bin") merge-base HEAD HEAD~$((commits / 2))"
    )
    if [[ "$gix_enabled" == "1" ]]; then
      specs+=($'gix\t'"$(shell_quote "$gix_bin") -r $(shell_quote "$src") merge-base HEAD HEAD~$((commits / 2))")
    fi
    run_group merge-base "$n" "$((seed + 400 + n))" "${specs[@]}"
  fi

  if benchmark_op_enabled pack-objects; then
    run_group pack-objects "$object_count objects" "$((seed + 500 + n))" \
      $'git\t'"cd $(shell_quote "$src") && $(shell_quote "$git_bin") pack-objects --stdout < $(shell_quote "$tmp_dir/objects.txt") > $(shell_quote "$tmp_dir/git-$n.pack")" \
      $'zmin\t'"cd $(shell_quote "$src") && $(shell_quote "$zmin_bin") pack-objects --stdout < $(shell_quote "$tmp_dir/objects.txt") > $(shell_quote "$tmp_dir/zmin-$n.pack")"
  elif benchmark_op_enabled index-pack; then
    "$git_bin" -C "$src" pack-objects --stdout <"$tmp_dir/objects.txt" >"$tmp_dir/git-$n.pack"
  fi

  if benchmark_op_enabled index-pack; then
    run_group index-pack "$n" "$((seed + 600 + n))" \
      $'git\t'"cd $(shell_quote "$tmp_dir") && rm -rf git-index-$n && $(shell_quote "$git_bin") init -q git-index-$n && $(shell_quote "$git_bin") -C git-index-$n index-pack --stdin < $(shell_quote "$tmp_dir/git-$n.pack")" \
      $'zmin\t'"cd $(shell_quote "$tmp_dir") && rm -rf zmin-index-$n && $(shell_quote "$git_bin") init -q zmin-index-$n && cd zmin-index-$n && $(shell_quote "$zmin_bin") index-pack --stdin < $(shell_quote "$tmp_dir/git-$n.pack")"
  fi
done

if any_benchmark_op_enabled add commit add-dirty commit-dirty; then
for n in $(seq 1 "$run_count"); do
  set_sample_policy "$n"
  git_repo="$tmp_dir/git-write-$n"
  zmin_repo="$tmp_dir/zmin-write-$n"
  "$git_bin" init -q -b main "$git_repo"
  "$zmin_bin" init "$zmin_repo" >/dev/null
  configure_repo "$git_repo"
  configure_repo "$zmin_repo"
  make_files "$git_repo" "$write_files" file
  make_files "$zmin_repo" "$write_files" file

  if benchmark_op_enabled add; then
    run_group add "$n/$write_files files" "$((seed + 700 + n))" \
      $'git\t'"cd $(shell_quote "$git_repo") && $(shell_quote "$git_bin") add -A" \
      $'zmin\t'"cd $(shell_quote "$zmin_repo") && $(shell_quote "$zmin_bin") add -A"
  elif any_benchmark_op_enabled commit add-dirty commit-dirty; then
    "$git_bin" -C "$git_repo" add -A
    "$zmin_bin" -C "$zmin_repo" add -A
  fi

  if benchmark_op_enabled commit; then
    run_group commit "$n/$write_files files" "$((seed + 800 + n))" \
      $'git\t'"cd $(shell_quote "$git_repo") && GIT_AUTHOR_DATE='1700000000 +0000' GIT_COMMITTER_DATE='1700000000 +0000' $(shell_quote "$git_bin") commit -qm initial" \
      $'zmin\t'"cd $(shell_quote "$zmin_repo") && GIT_AUTHOR_DATE='1700000000 +0000' GIT_COMMITTER_DATE='1700000000 +0000' $(shell_quote "$zmin_bin") commit -qm initial"
  elif any_benchmark_op_enabled add-dirty commit-dirty; then
    GIT_AUTHOR_DATE='1700000000 +0000' GIT_COMMITTER_DATE='1700000000 +0000' \
      "$git_bin" -C "$git_repo" commit -qm initial
    GIT_AUTHOR_DATE='1700000000 +0000' GIT_COMMITTER_DATE='1700000000 +0000' \
      "$zmin_bin" -C "$zmin_repo" commit -qm initial >/dev/null
  fi

  if any_benchmark_op_enabled commit add-dirty commit-dirty; then
    "$git_bin" -C "$zmin_repo" fsck --strict >/dev/null
    compare_trees "commit-$n" "$git_repo" "$zmin_repo" HEAD
  fi

  if any_benchmark_op_enabled add-dirty commit-dirty; then
    for i in $(seq 1 "$dirty_files"); do
      printf 'changed %05d\n' "$i" >>"$git_repo/dir-$((i % 32))/file-$i.txt"
      printf 'changed %05d\n' "$i" >>"$zmin_repo/dir-$((i % 32))/file-$i.txt"
    done
    if benchmark_op_enabled add-dirty; then
      run_group add-dirty "$n/$dirty_files files" "$((seed + 900 + n))" \
        $'git\t'"cd $(shell_quote "$git_repo") && $(shell_quote "$git_bin") add -A" \
        $'zmin\t'"cd $(shell_quote "$zmin_repo") && $(shell_quote "$zmin_bin") add -A"
    elif benchmark_op_enabled commit-dirty; then
      "$git_bin" -C "$git_repo" add -A
      "$zmin_bin" -C "$zmin_repo" add -A
    fi
    if benchmark_op_enabled commit-dirty; then
      run_group commit-dirty "$n/$dirty_files files" "$((seed + 1000 + n))" \
        $'git\t'"cd $(shell_quote "$git_repo") && GIT_AUTHOR_DATE='1700000001 +0000' GIT_COMMITTER_DATE='1700000001 +0000' $(shell_quote "$git_bin") commit -qm dirty" \
        $'zmin\t'"cd $(shell_quote "$zmin_repo") && GIT_AUTHOR_DATE='1700000001 +0000' GIT_COMMITTER_DATE='1700000001 +0000' $(shell_quote "$zmin_bin") commit -qm dirty"
      "$git_bin" -C "$zmin_repo" fsck --strict >/dev/null
      compare_trees "commit-dirty-$n" "$git_repo" "$zmin_repo" HEAD
    fi
  fi
done
fi

clone_large_src=""
if benchmark_op_enabled clone-large; then
  clone_large_src="$tmp_dir/src-clone-large"
  create_source_repo "$clone_large_src" "$clone_large_commits" "$clone_large_files_per_commit"
fi

if any_benchmark_op_enabled clone clone-large clone-instant; then
  for n in $(seq 1 "$run_count"); do
    set_sample_policy "$n"
    if benchmark_op_enabled clone; then
      clone_specs=(
        $'git\t'"$(shell_quote "$git_bin") clone -q $(shell_quote "$src") $(shell_quote "$tmp_dir/git-clone-$n")"
        $'zmin\t'"$(shell_quote "$zmin_bin") clone -q $(shell_quote "$src") $(shell_quote "$tmp_dir/zmin-clone-$n")"
      )
      if [[ "$gix_enabled" == "1" ]]; then
        clone_specs+=($'gix\t'"$(shell_quote "$gix_bin") clone $(shell_quote "$src") $(shell_quote "$tmp_dir/gix-clone-$n")")
      fi
      run_group clone "$n/local" "$((seed + 1100 + n))" "${clone_specs[@]}"
      compare_refs "clone-$n" "$tmp_dir/git-clone-$n" "$tmp_dir/zmin-clone-$n" HEAD
      compare_trees "clone-$n-tree" "$tmp_dir/git-clone-$n" "$tmp_dir/zmin-clone-$n" HEAD
    fi

    if benchmark_op_enabled clone-large; then
      clone_large_specs=(
        $'git\t'"$(shell_quote "$git_bin") clone -q $(shell_quote "$clone_large_src") $(shell_quote "$tmp_dir/git-clone-large-$n")"
        $'zmin\t'"$(shell_quote "$zmin_bin") clone -q $(shell_quote "$clone_large_src") $(shell_quote "$tmp_dir/zmin-clone-large-$n")"
      )
      if [[ "$gix_enabled" == "1" ]]; then
        clone_large_specs+=($'gix\t'"$(shell_quote "$gix_bin") clone $(shell_quote "$clone_large_src") $(shell_quote "$tmp_dir/gix-clone-large-$n")")
      fi
      run_group clone-large "$n/$clone_large_commits commits/$clone_large_files_per_commit files" "$((seed + 1125 + n))" "${clone_large_specs[@]}"
      compare_refs "clone-large-$n" "$tmp_dir/git-clone-large-$n" "$tmp_dir/zmin-clone-large-$n" HEAD
      compare_trees "clone-large-$n-tree" "$tmp_dir/git-clone-large-$n" "$tmp_dir/zmin-clone-large-$n" HEAD
    fi

    if benchmark_op_enabled clone-instant; then
      run_group clone-instant "$n/local" "$((seed + 1150 + n))" \
        $'git\t'"$(shell_quote "$git_bin") clone -q $(shell_quote "$src") $(shell_quote "$tmp_dir/git-clone-instant-$n")" \
        $'zmin\t'"$(shell_quote "$zmin_bin") clone -q --instant $(shell_quote "$src") $(shell_quote "$tmp_dir/zmin-clone-instant-$n")"
      compare_refs "clone-instant-$n" "$tmp_dir/git-clone-instant-$n" "$tmp_dir/zmin-clone-instant-$n" HEAD
      compare_trees "clone-instant-$n-tree" "$tmp_dir/git-clone-instant-$n" "$tmp_dir/zmin-clone-instant-$n" HEAD
      check_worktree_first_marker "clone-instant-$n-marker" "$tmp_dir/zmin-clone-instant-$n"
    fi
  done
fi

if any_benchmark_op_enabled clone-instant-git-daemon clone-instant-ssh; then
daemon_remote="$tmp_dir/daemon-remote.git"
"$git_bin" clone -q --bare "$src" "$daemon_remote"
"$git_bin" --git-dir "$daemon_remote" symbolic-ref HEAD refs/heads/main
touch "$daemon_remote/git-daemon-export-ok"
daemon_port="$(unused_local_port)"
daemon_url="git://127.0.0.1:$daemon_port/daemon-remote.git"
start_git_daemon "$tmp_dir" "$daemon_url" "$daemon_port"

ssh_remote="$tmp_dir/ssh-remote.git"
"$git_bin" clone -q --bare "$src" "$ssh_remote"
"$git_bin" --git-dir "$ssh_remote" symbolic-ref HEAD refs/heads/main
fake_ssh="$(write_fake_ssh)"
fake_ssh_git_exec_path="$("$git_bin" --exec-path)"
fake_ssh_env="ZMIN_BENCH_FAKE_SSH_GIT_EXEC_PATH=$(shell_quote "$fake_ssh_git_exec_path") GIT_SSH_COMMAND=$(shell_quote "$fake_ssh")"
ssh_url="ssh://example.test$ssh_remote"

for n in $(seq 1 "$run_count"); do
  set_sample_policy "$n"
  if benchmark_op_enabled clone-instant-git-daemon; then
    run_group clone-instant-git-daemon "$n/git-daemon" "$((seed + 1160 + n))" \
      $'git\t'"$(shell_quote "$git_bin") clone -q $(shell_quote "$daemon_url") $(shell_quote "$tmp_dir/git-daemon-instant-baseline-$n")" \
      $'zmin\t'"$(shell_quote "$zmin_bin") clone -q --instant $(shell_quote "$daemon_url") $(shell_quote "$tmp_dir/zmin-daemon-instant-$n")"
    "$git_bin" -C "$tmp_dir/zmin-daemon-instant-$n" fsck --strict >/dev/null
    compare_refs "clone-instant-git-daemon-$n" \
      "$tmp_dir/git-daemon-instant-baseline-$n" \
      "$tmp_dir/zmin-daemon-instant-$n" \
      HEAD
    compare_trees "clone-instant-git-daemon-$n-tree" \
      "$tmp_dir/git-daemon-instant-baseline-$n" \
      "$tmp_dir/zmin-daemon-instant-$n" \
      HEAD
    check_worktree_first_marker "clone-instant-git-daemon-$n-marker" "$tmp_dir/zmin-daemon-instant-$n"
  fi

  if benchmark_op_enabled clone-instant-ssh; then
    run_group clone-instant-ssh "$n/ssh" "$((seed + 1170 + n))" \
      $'git\t'"$fake_ssh_env $(shell_quote "$git_bin") clone -q $(shell_quote "$ssh_url") $(shell_quote "$tmp_dir/git-ssh-instant-baseline-$n")" \
      $'zmin\t'"$fake_ssh_env $(shell_quote "$zmin_bin") clone -q --instant $(shell_quote "$ssh_url") $(shell_quote "$tmp_dir/zmin-ssh-instant-$n")"
    "$git_bin" -C "$tmp_dir/zmin-ssh-instant-$n" fsck --strict >/dev/null
    compare_refs "clone-instant-ssh-$n" \
      "$tmp_dir/git-ssh-instant-baseline-$n" \
      "$tmp_dir/zmin-ssh-instant-$n" \
      HEAD
    compare_trees "clone-instant-ssh-$n-tree" \
      "$tmp_dir/git-ssh-instant-baseline-$n" \
      "$tmp_dir/zmin-ssh-instant-$n" \
      HEAD
    check_worktree_first_marker "clone-instant-ssh-$n-marker" "$tmp_dir/zmin-ssh-instant-$n"
  fi
done
fi

if any_benchmark_op_enabled push-noop push-incremental; then
push_remote="$tmp_dir/push-remote.git"
"$git_bin" init -q --bare "$push_remote"
"$git_bin" clone -q "$src" "$tmp_dir/git-push-base"
"$zmin_bin" clone -q "$src" "$tmp_dir/zmin-push-base" >/dev/null
"$git_bin" -C "$tmp_dir/git-push-base" remote remove origin
"$git_bin" -C "$tmp_dir/zmin-push-base" remote remove origin
"$git_bin" -C "$tmp_dir/git-push-base" remote add origin "$push_remote"
"$git_bin" -C "$tmp_dir/zmin-push-base" remote add origin "$push_remote"
"$git_bin" -C "$tmp_dir/git-push-base" push -q origin main
if benchmark_op_enabled push-noop; then
  for n in $(seq 1 "$run_count"); do
    set_sample_policy "$n"
    run_group push-noop "$n/remote" "$((seed + 1200 + n))" \
      $'git\t'"cd $(shell_quote "$tmp_dir/git-push-base") && $(shell_quote "$git_bin") push origin main" \
      $'zmin\t'"cd $(shell_quote "$tmp_dir/zmin-push-base") && $(shell_quote "$zmin_bin") push origin main"
  done
fi

if benchmark_op_enabled push-incremental; then
  printf 'incremental\n' >"$tmp_dir/git-push-base/incremental.txt"
  printf 'incremental\n' >"$tmp_dir/zmin-push-base/incremental.txt"
  "$git_bin" -C "$tmp_dir/git-push-base" add -A
  "$git_bin" -C "$tmp_dir/zmin-push-base" add -A
  GIT_AUTHOR_DATE='1700080000 +0000' GIT_COMMITTER_DATE='1700080000 +0000' \
    "$git_bin" -C "$tmp_dir/git-push-base" commit -qm incremental
  GIT_AUTHOR_DATE='1700080000 +0000' GIT_COMMITTER_DATE='1700080000 +0000' \
    "$zmin_bin" -C "$tmp_dir/zmin-push-base" commit -qm incremental >/dev/null
  compare_trees push-incremental-prep "$tmp_dir/git-push-base" "$tmp_dir/zmin-push-base" HEAD
  for n in $(seq 1 "$run_count"); do
    set_sample_policy "$n"
    run_group push-incremental "$n/remote" "$((seed + 1300 + n))" \
      $'git\t'"cd $(shell_quote "$tmp_dir/git-push-base") && $(shell_quote "$git_bin") push origin HEAD:refs/heads/git-incremental-$n" \
      $'zmin\t'"cd $(shell_quote "$tmp_dir/zmin-push-base") && $(shell_quote "$zmin_bin") push origin HEAD:refs/heads/zmin-incremental-$n"
    "$git_bin" --git-dir "$push_remote" rev-parse "refs/heads/git-incremental-$n" >/dev/null
    "$git_bin" --git-dir "$push_remote" rev-parse "refs/heads/zmin-incremental-$n" >/dev/null
  done
  record_validation push-incremental ok refs_present
fi
fi

if benchmark_op_enabled push-batch; then
push_batch_remote="$tmp_dir/push-batch-remote.git"
"$git_bin" init -q --bare "$push_batch_remote"
"$git_bin" clone -q "$src" "$tmp_dir/git-push-batch-base"
"$zmin_bin" clone -q "$src" "$tmp_dir/zmin-push-batch-base" >/dev/null
"$git_bin" -C "$tmp_dir/git-push-batch-base" remote remove origin
"$git_bin" -C "$tmp_dir/zmin-push-batch-base" remote remove origin
"$git_bin" -C "$tmp_dir/git-push-batch-base" remote add origin "$push_batch_remote"
"$git_bin" -C "$tmp_dir/zmin-push-batch-base" remote add origin "$push_batch_remote"
"$git_bin" -C "$tmp_dir/git-push-batch-base" push -q origin main
for n in $(seq 1 "$run_count"); do
  set_sample_policy "$n"
  cp -R "$tmp_dir/git-push-batch-base" "$tmp_dir/git-push-batch-$n"
  cp -R "$tmp_dir/zmin-push-batch-base" "$tmp_dir/zmin-push-batch-$n"
  mkdir -p "$tmp_dir/git-push-batch-$n/push-batch" "$tmp_dir/zmin-push-batch-$n/push-batch"
  for i in $(seq 1 "$push_batch_files"); do
    printf 'push batch %04d %04096d\n' "$i" 0 >"$tmp_dir/git-push-batch-$n/push-batch/file-$i.txt"
    printf 'push batch %04d %04096d\n' "$i" 0 >"$tmp_dir/zmin-push-batch-$n/push-batch/file-$i.txt"
  done
  "$git_bin" -C "$tmp_dir/git-push-batch-$n" add -A
  "$git_bin" -C "$tmp_dir/zmin-push-batch-$n" add -A
  ts=$((1700081000 + n))
  GIT_AUTHOR_DATE="$ts +0000" GIT_COMMITTER_DATE="$ts +0000" \
    "$git_bin" -C "$tmp_dir/git-push-batch-$n" commit -qm push-batch
  GIT_AUTHOR_DATE="$ts +0000" GIT_COMMITTER_DATE="$ts +0000" \
    "$zmin_bin" -C "$tmp_dir/zmin-push-batch-$n" commit -qm push-batch >/dev/null
  compare_trees "push-batch-prep-$n" "$tmp_dir/git-push-batch-$n" "$tmp_dir/zmin-push-batch-$n" HEAD
  run_group push-batch "$n/$push_batch_files files" "$((seed + 1400 + n))" \
    $'git\t'"cd $(shell_quote "$tmp_dir/git-push-batch-$n") && $(shell_quote "$git_bin") push origin HEAD:refs/heads/git-push-batch-$n" \
    $'zmin\t'"cd $(shell_quote "$tmp_dir/zmin-push-batch-$n") && $(shell_quote "$zmin_bin") push origin HEAD:refs/heads/zmin-push-batch-$n"
done
record_validation push-batch ok refs_pushed
fi

if any_benchmark_op_enabled pull-noop pull-incremental; then
pull_remote="$tmp_dir/pull-remote.git"
pull_src="$tmp_dir/pull-source"
"$git_bin" init -q --bare "$pull_remote"
"$git_bin" clone -q "$src" "$pull_src"
configure_repo "$pull_src"
"$git_bin" -C "$pull_src" remote remove origin
"$git_bin" -C "$pull_src" remote add origin "$pull_remote"
"$git_bin" -C "$pull_src" push -q origin main
"$git_bin" --git-dir "$pull_remote" symbolic-ref HEAD refs/heads/main
"$git_bin" clone -q "$pull_remote" "$tmp_dir/git-pull-base"
"$zmin_bin" clone -q "$pull_remote" "$tmp_dir/zmin-pull-base" >/dev/null
configure_repo "$tmp_dir/git-pull-base"
configure_repo "$tmp_dir/zmin-pull-base"
if benchmark_op_enabled pull-noop; then
  for n in $(seq 1 "$run_count"); do
    set_sample_policy "$n"
    run_group pull-noop "$n/remote" "$((seed + 1450 + n))" \
      $'git\t'"cd $(shell_quote "$tmp_dir/git-pull-base") && $(shell_quote "$git_bin") pull --ff-only" \
      $'zmin\t'"cd $(shell_quote "$tmp_dir/zmin-pull-base") && $(shell_quote "$zmin_bin") pull --ff-only"
  done
  compare_refs pull-noop "$tmp_dir/git-pull-base" "$tmp_dir/zmin-pull-base" HEAD
fi

if benchmark_op_enabled pull-incremental; then
  printf 'pull incremental\n' >"$pull_src/pull-incremental.txt"
  "$git_bin" -C "$pull_src" add -A
  GIT_AUTHOR_DATE='1700085000 +0000' GIT_COMMITTER_DATE='1700085000 +0000' \
    "$git_bin" -C "$pull_src" commit -qm pull-incremental
  "$git_bin" -C "$pull_src" push -q origin main
  for n in $(seq 1 "$run_count"); do
    set_sample_policy "$n"
    cp -R "$tmp_dir/git-pull-base" "$tmp_dir/git-pull-incremental-$n"
    cp -R "$tmp_dir/zmin-pull-base" "$tmp_dir/zmin-pull-incremental-$n"
    run_group pull-incremental "$n/remote" "$((seed + 1475 + n))" \
      $'git\t'"cd $(shell_quote "$tmp_dir/git-pull-incremental-$n") && $(shell_quote "$git_bin") pull --ff-only" \
      $'zmin\t'"cd $(shell_quote "$tmp_dir/zmin-pull-incremental-$n") && $(shell_quote "$zmin_bin") pull --ff-only"
    compare_refs "pull-incremental-$n" \
      "$tmp_dir/git-pull-incremental-$n" \
      "$tmp_dir/zmin-pull-incremental-$n" \
      HEAD
    compare_refs "pull-incremental-source-$n" \
      "$pull_src" \
      "$tmp_dir/zmin-pull-incremental-$n" \
      HEAD
  done
fi
fi

if benchmark_op_enabled fetch-noop; then
  specs=(
    $'git\t'"cd $(shell_quote "$tmp_dir/git-fetch") && $(shell_quote "$git_bin") fetch origin"
    $'zmin\t'"cd $(shell_quote "$tmp_dir/zmin-fetch") && $(shell_quote "$zmin_bin") fetch origin"
  )
  if [[ "$gix_enabled" == "1" ]]; then
    specs+=($'gix\t'"$(shell_quote "$gix_bin") -r $(shell_quote "$tmp_dir/gix-fetch") fetch -r origin")
  fi
  for n in $(seq 1 "$run_count"); do
    set_sample_policy "$n"
    run_group fetch-noop "$n/remote" "$((seed + 1500 + n))" "${specs[@]}"
  done
  compare_refs fetch-noop "$tmp_dir/git-fetch" "$tmp_dir/zmin-fetch" refs/remotes/origin/main
fi

if benchmark_op_enabled fetch-incremental; then
  for n in $(seq 1 "$run_count"); do
    set_sample_policy "$n"
    cp -R "$tmp_dir/git-fetch" "$tmp_dir/git-fetch-incremental-$n"
    cp -R "$tmp_dir/zmin-fetch" "$tmp_dir/zmin-fetch-incremental-$n"
    "$git_bin" -C "$tmp_dir/git-fetch-incremental-$n" remote set-url origin "$fetch_incremental_remote"
    "$git_bin" -C "$tmp_dir/zmin-fetch-incremental-$n" remote set-url origin "$fetch_incremental_remote"
    specs=(
      $'git\t'"cd $(shell_quote "$tmp_dir/git-fetch-incremental-$n") && $(shell_quote "$git_bin") fetch origin"
      $'zmin\t'"cd $(shell_quote "$tmp_dir/zmin-fetch-incremental-$n") && $(shell_quote "$zmin_bin") fetch origin"
    )
    if [[ "$gix_enabled" == "1" ]]; then
      cp -R "$tmp_dir/gix-fetch" "$tmp_dir/gix-fetch-incremental-$n"
      "$git_bin" -C "$tmp_dir/gix-fetch-incremental-$n" remote set-url origin "$fetch_incremental_remote"
      specs+=($'gix\t'"$(shell_quote "$gix_bin") -r $(shell_quote "$tmp_dir/gix-fetch-incremental-$n") fetch -r origin")
    fi
    run_group fetch-incremental "$n/remote" "$((seed + 1600 + n))" "${specs[@]}"
    compare_refs "fetch-incremental-$n" "$tmp_dir/git-fetch-incremental-$n" "$tmp_dir/zmin-fetch-incremental-$n" refs/remotes/origin/main
  done
fi

if benchmark_op_enabled fetch-batch; then
for n in $(seq 1 "$run_count"); do
  set_sample_policy "$n"
  cp -R "$tmp_dir/git-fetch-batch-base" "$tmp_dir/git-fetch-batch-$n"
  cp -R "$tmp_dir/zmin-fetch-batch-base" "$tmp_dir/zmin-fetch-batch-$n"
  specs=(
    $'git\t'"cd $(shell_quote "$tmp_dir/git-fetch-batch-$n") && $(shell_quote "$git_bin") fetch origin"
    $'zmin\t'"cd $(shell_quote "$tmp_dir/zmin-fetch-batch-$n") && $(shell_quote "$zmin_bin") fetch origin"
  )
  if [[ "$gix_enabled" == "1" ]]; then
    cp -R "$tmp_dir/gix-fetch-batch-base" "$tmp_dir/gix-fetch-batch-$n"
    specs+=($'gix\t'"$(shell_quote "$gix_bin") -r $(shell_quote "$tmp_dir/gix-fetch-batch-$n") fetch -r origin")
  fi
  run_group fetch-batch "$n/$fetch_batch_files files" "$((seed + 1700 + n))" "${specs[@]}"
  "$git_bin" -C "$tmp_dir/zmin-fetch-batch-$n" fsck --strict >/dev/null
  compare_refs "fetch-batch-$n" "$tmp_dir/git-fetch-batch-$n" "$tmp_dir/zmin-fetch-batch-$n" refs/remotes/origin/main
done
fi

artifact_read_root "$tmp_dir" "$tmp_artifact_identity" bench.tsv
artifact_read_root "$tmp_dir" "$tmp_artifact_identity" validation.tsv
if [[ -f "$tmp_dir/git-1.pack" ]]; then
  printf 'pack_bytes\tgit\t%s\n' "$(wc -c <"$tmp_dir/git-1.pack" | tr -d ' ')"
fi
if [[ -f "$tmp_dir/zmin-1.pack" ]]; then
  printf 'pack_bytes\tzmin\t%s\n' "$(wc -c <"$tmp_dir/zmin-1.pack" | tr -d ' ')"
fi

if [[ -n "$out_dir" ]]; then
  rows_path="$out_dir/bench.tsv"
  checks_path="$out_dir/checks.tsv"
  summary_path="$out_dir/summary.csv"
  comparison_path="$out_dir/comparison.csv"
  artifact_copy_root "$out_dir" "$evidence_artifact_identity" bench.tsv "$out"
  artifact_copy_root "$out_dir" "$evidence_artifact_identity" checks.tsv "$validation_out"
  "$python_bin" - "$rows_path" "$summary_path" "$comparison_path" \
    "$evidence_dir" "$evidence_artifact_identity" "$repo_root/tools" <<'PY'
import csv
import io
import math
import os
import pathlib
import statistics
import sys
from collections import defaultdict

rows_path, summary_path, comparison_path, evidence_dir, evidence_identity, tools_dir = sys.argv[1:7]
sys.path.insert(0, tools_dir)
import performance_contract as contract

rows_by_op_tool = defaultdict(list)
rows_by_op_tool_extra = defaultdict(dict)
memory_by_op_tool = defaultdict(list)
memory_identity_by_op = {}
rows_data = contract.artifact_read_bytes(
    pathlib.Path(evidence_dir),
    contract.artifact_relative_name(pathlib.Path(evidence_dir), pathlib.Path(rows_path)),
    expected_directory_identity=contract.parse_artifact_identity(evidence_identity),
).decode()
for row in csv.DictReader(io.StringIO(rows_data), delimiter="\t"):
    tool = row.get("tool", "")
    if tool not in {"git", "zmin", "gix"}:
        continue
    if row.get("sample_kind", "measured") != "measured":
        continue
    try:
        seconds = float(row["real"])
    except (KeyError, TypeError, ValueError):
        continue
    op = row["op"]
    rows_by_op_tool[(op, tool)].append(seconds)
    rows_by_op_tool_extra[(op, tool)][row.get("extra", "")] = seconds
    memory_metric = row.get("memory_metric", "")
    memory_semantics = row.get("memory_semantics", "")
    memory_scope = row.get("memory_scope", "")
    memory_unit = row.get("memory_unit", "")
    identity = (memory_metric, memory_semantics, memory_scope, memory_unit)
    if identity not in {("peak_rss_bytes", "working_set_peak", "waited_child_processes", "bytes"), ("peak_job_commit_bytes", "job_commit_peak", "job_process_tree", "bytes")}:
        continue
    previous_identity = memory_identity_by_op.get(op)
    if previous_identity is not None and previous_identity != identity:
        raise SystemExit(f"inconsistent memory metric identity for {op}")
    memory_identity_by_op[op] = identity
    memory_field = "rss_bytes" if memory_metric == "peak_rss_bytes" else "job_commit_bytes"
    try:
        memory_bytes = int(row[memory_field])
    except (KeyError, TypeError, ValueError):
        memory_bytes = None
    if memory_bytes is not None and memory_bytes > 0:
        memory_by_op_tool[(op, tool)].append(memory_bytes)


def rounded(value):
    return f"{value:.6f}"


def ratio(numerator, denominator):
    if denominator == 0:
        return ""
    return rounded(numerator / denominator)


def percentile(values, fraction):
    ordered = sorted(values)
    index = max(0, math.ceil(len(ordered) * fraction) - 1)
    return ordered[index]


def paired_ratios(op, numerator_tool, denominator_tool):
    numerator = rows_by_op_tool_extra.get((op, numerator_tool), {})
    denominator = rows_by_op_tool_extra.get((op, denominator_tool), {})
    values = []
    for extra in sorted(set(numerator) & set(denominator)):
        denominator_value = denominator[extra]
        if denominator_value != 0:
            values.append(numerator[extra] / denominator_value)
    return sorted(values)


summary_rows = []
for (op, tool), values in sorted(rows_by_op_tool.items()):
    values = sorted(values)
    memory_values = sorted(memory_by_op_tool.get((op, tool), []))
    memory_identity = memory_identity_by_op.get(op, ("", "", "", ""))
    summary_rows.append(
        {
            "op": op,
            "tool": tool,
            "runs": str(len(values)),
            "mean_seconds": rounded(statistics.mean(values)),
            "median_seconds": rounded(statistics.median(values)),
            "min_seconds": rounded(values[0]),
            "max_seconds": rounded(values[-1]),
            "memory_metric": memory_identity[0],
            "memory_semantics": memory_identity[1],
            "memory_scope": memory_identity[2],
            "memory_unit": memory_identity[3],
            "median_memory_bytes": "" if not memory_values else str(int(statistics.median(memory_values))),
            "p95_memory_bytes": "" if not memory_values else str(percentile(memory_values, 0.95)),
        }
    )

summary_buffer = io.StringIO(newline="")
fieldnames = [
    "op",
    "tool",
    "runs",
    "mean_seconds",
    "median_seconds",
    "min_seconds",
    "max_seconds",
    "memory_metric",
    "memory_semantics",
    "memory_scope",
    "memory_unit",
    "median_memory_bytes",
    "p95_memory_bytes",
]
writer = csv.DictWriter(summary_buffer, fieldnames=fieldnames)
writer.writeheader()
writer.writerows(summary_rows)
contract.artifact_write_bytes(
    pathlib.Path(evidence_dir),
    contract.artifact_relative_name(pathlib.Path(evidence_dir), pathlib.Path(summary_path)),
    summary_buffer.getvalue().encode(),
    expected_directory_identity=contract.parse_artifact_identity(evidence_identity),
)

ops = sorted({op for op, _ in rows_by_op_tool})
comparison_rows = []
for op in ops:
    git = sorted(rows_by_op_tool.get((op, "git"), []))
    zmin = sorted(rows_by_op_tool.get((op, "zmin"), []))
    gix = sorted(rows_by_op_tool.get((op, "gix"), []))
    git_memory = sorted(memory_by_op_tool.get((op, "git"), []))
    zmin_memory = sorted(memory_by_op_tool.get((op, "zmin"), []))
    if not git or not zmin:
        continue
    git_mean = statistics.mean(git)
    zmin_mean = statistics.mean(zmin)
    git_median = statistics.median(git)
    zmin_median = statistics.median(zmin)
    gix_mean = statistics.mean(gix) if gix else None
    gix_median = statistics.median(gix) if gix else None
    zmin_git_pairs = paired_ratios(op, "zmin", "git")
    zmin_gix_pairs = paired_ratios(op, "zmin", "gix")
    git_memory_p95 = percentile(git_memory, 0.95) if git_memory else None
    zmin_memory_p95 = percentile(zmin_memory, 0.95) if zmin_memory else None
    comparison_rows.append(
        {
            "op": op,
            "runs": str(min(len(git), len(zmin))),
            "git_mean_seconds": rounded(git_mean),
            "zmin_mean_seconds": rounded(zmin_mean),
            "zmin_vs_git_mean_ratio": ratio(zmin_mean, git_mean),
            "gix_mean_seconds": "" if gix_mean is None else rounded(gix_mean),
            "zmin_vs_gix_mean_ratio": "" if gix_mean is None else ratio(zmin_mean, gix_mean),
            "git_median_seconds": rounded(git_median),
            "zmin_median_seconds": rounded(zmin_median),
            "zmin_vs_git_median_ratio": ratio(zmin_median, git_median),
            "gix_median_seconds": "" if gix_median is None else rounded(gix_median),
            "zmin_vs_gix_median_ratio": ""
            if gix_median is None
            else ratio(zmin_median, gix_median),
            "zmin_vs_git_pair_count": str(len(zmin_git_pairs)),
            "zmin_vs_git_pair_mean_ratio": ""
            if not zmin_git_pairs
            else rounded(statistics.mean(zmin_git_pairs)),
            "zmin_vs_git_pair_median_ratio": ""
            if not zmin_git_pairs
            else rounded(statistics.median(zmin_git_pairs)),
            "zmin_vs_git_pair_min_ratio": ""
            if not zmin_git_pairs
            else rounded(zmin_git_pairs[0]),
            "zmin_vs_git_pair_max_ratio": ""
            if not zmin_git_pairs
            else rounded(zmin_git_pairs[-1]),
            "zmin_vs_gix_pair_count": "" if not zmin_gix_pairs else str(len(zmin_gix_pairs)),
            "zmin_vs_gix_pair_mean_ratio": ""
            if not zmin_gix_pairs
            else rounded(statistics.mean(zmin_gix_pairs)),
            "zmin_vs_gix_pair_median_ratio": ""
            if not zmin_gix_pairs
            else rounded(statistics.median(zmin_gix_pairs)),
            "memory_metric": memory_identity_by_op.get(op, ("", "", "", ""))[0],
            "memory_semantics": memory_identity_by_op.get(op, ("", "", "", ""))[1],
            "memory_scope": memory_identity_by_op.get(op, ("", "", "", ""))[2],
            "memory_unit": memory_identity_by_op.get(op, ("", "", "", ""))[3],
            "git_p95_memory_bytes": "" if git_memory_p95 is None else str(git_memory_p95),
            "zmin_p95_memory_bytes": "" if zmin_memory_p95 is None else str(zmin_memory_p95),
            "zmin_vs_git_p95_memory_ratio": ""
            if git_memory_p95 is None or zmin_memory_p95 is None
            else ratio(zmin_memory_p95, git_memory_p95),
        }
    )

comparison_buffer = io.StringIO(newline="")
fieldnames = [
    "op",
    "runs",
    "git_mean_seconds",
    "zmin_mean_seconds",
    "zmin_vs_git_mean_ratio",
    "gix_mean_seconds",
    "zmin_vs_gix_mean_ratio",
    "git_median_seconds",
    "zmin_median_seconds",
    "zmin_vs_git_median_ratio",
    "gix_median_seconds",
    "zmin_vs_gix_median_ratio",
    "zmin_vs_git_pair_count",
    "zmin_vs_git_pair_mean_ratio",
    "zmin_vs_git_pair_median_ratio",
    "zmin_vs_git_pair_min_ratio",
    "zmin_vs_git_pair_max_ratio",
    "zmin_vs_gix_pair_count",
    "zmin_vs_gix_pair_mean_ratio",
    "zmin_vs_gix_pair_median_ratio",
    "memory_metric",
    "memory_semantics",
    "memory_scope",
    "memory_unit",
    "git_p95_memory_bytes",
    "zmin_p95_memory_bytes",
    "zmin_vs_git_p95_memory_ratio",
]
writer = csv.DictWriter(comparison_buffer, fieldnames=fieldnames)
writer.writeheader()
writer.writerows(comparison_rows)
contract.artifact_write_bytes(
    pathlib.Path(evidence_dir),
    contract.artifact_relative_name(pathlib.Path(evidence_dir), pathlib.Path(comparison_path)),
    comparison_buffer.getvalue().encode(),
    expected_directory_identity=contract.parse_artifact_identity(evidence_identity),
)


def max_ratio_from_env(name):
    value = os.environ.get(name, "")
    if not value:
        return 0.0
    try:
        return float(value)
    except ValueError:
        raise SystemExit(f"{name} must be a number")


def assert_max_ratio(column, max_ratio, label):
    if max_ratio <= 0.0:
        return
    failures = []
    for row in comparison_rows:
        value = row.get(column, "")
        if value == "":
            failures.append(f"{row['op']}: missing {label}")
            continue
        ratio_value = float(value)
        if ratio_value > max_ratio:
            failures.append(f"{row['op']}: {label} {ratio_value:.6f} > {max_ratio:.6f}")
    if failures:
        raise SystemExit(f"benchmark ratio gate failed for {label}: {'; '.join(failures)}")


assert_max_ratio(
    "zmin_vs_git_mean_ratio",
    max_ratio_from_env("ZMIN_BENCH_MAX_ZMIN_VS_GIT_MEAN_RATIO"),
    "Zmin/Git mean",
)
assert_max_ratio(
    "zmin_vs_git_median_ratio",
    max_ratio_from_env("ZMIN_BENCH_MAX_ZMIN_VS_GIT_MEDIAN_RATIO"),
    "Zmin/Git median",
)
assert_max_ratio(
    "zmin_vs_git_pair_median_ratio",
    max_ratio_from_env("ZMIN_BENCH_MAX_ZMIN_VS_GIT_PAIR_MEDIAN_RATIO"),
    "Zmin/Git paired median",
)
assert_max_ratio(
    "zmin_vs_git_p95_memory_ratio",
    max_ratio_from_env("ZMIN_BENCH_MAX_ZMIN_VS_GIT_P95_MEMORY_RATIO"),
    "Zmin/Git p95 memory",
)
assert_max_ratio(
    "zmin_vs_gix_mean_ratio",
    max_ratio_from_env("ZMIN_BENCH_MAX_ZMIN_VS_GIX_MEAN_RATIO"),
    "Zmin/Gitoxide mean",
)
assert_max_ratio(
    "zmin_vs_gix_median_ratio",
    max_ratio_from_env("ZMIN_BENCH_MAX_ZMIN_VS_GIX_MEDIAN_RATIO"),
    "Zmin/Gitoxide median",
)
assert_max_ratio(
    "zmin_vs_gix_pair_median_ratio",
    max_ratio_from_env("ZMIN_BENCH_MAX_ZMIN_VS_GIX_PAIR_MEDIAN_RATIO"),
    "Zmin/Gitoxide paired median",
)
PY
  if [[ "$evidence_mode" == "authoritative" ]]; then
    artifact_cli superiority-summary \
      --metadata "$metadata_path" \
      --rows "$rows_path" \
      --output "$out_dir/superiority.tsv" \
      --results-dir "$out_dir" \
      --root-identity "$evidence_artifact_identity"
  fi
  if [[ "$evidence_mode" == "authoritative" ]]; then
    "$python_bin" "$repo_root/tools/performance_contract.py" finish \
      --metadata "$metadata_path" \
      --rows "$rows_path" \
      --output "$evidence_dir/evidence.json" \
      --results-dir "$evidence_dir" \
      --require-authoritative \
      "${finish_anchor_args[@]}"
  else
    "$python_bin" "$repo_root/tools/performance_contract.py" finish \
      --metadata "$metadata_path" \
      --rows "$rows_path" \
      --output "$evidence_dir/evidence.json" \
      --results-dir "$evidence_dir" \
      "${finish_anchor_args[@]}"
  fi
  printf 'rows=%s\n' "$rows_path" >&2
  printf 'checks=%s\n' "$checks_path" >&2
  printf 'summary=%s\n' "$summary_path" >&2
  printf 'comparison=%s\n' "$comparison_path" >&2
  else
  if [[ "$evidence_mode" == "authoritative" ]]; then
    artifact_cli superiority-summary \
      --metadata "$metadata_path" \
      --rows "$out" \
      --output "$evidence_dir/superiority.tsv" \
      --results-dir "$evidence_dir" \
      --root-identity "$evidence_artifact_identity"
  fi
  if [[ "$evidence_mode" == "authoritative" ]]; then
    "$python_bin" "$repo_root/tools/performance_contract.py" finish \
      --metadata "$metadata_path" \
      --rows "$out" \
      --output "$evidence_dir/evidence.json" \
      --result "$out" \
      --result "$validation_out" \
      --require-authoritative \
      "${finish_anchor_args[@]}"
  else
    "$python_bin" "$repo_root/tools/performance_contract.py" finish \
      --metadata "$metadata_path" \
      --rows "$out" \
      --output "$evidence_dir/evidence.json" \
      --result "$out" \
      --result "$validation_out" \
      "${finish_anchor_args[@]}"
  fi
fi
