#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
git_bin="${GIT_BIN:-$(command -v git)}"
zmin_bin="${ZMIN_BIN:-$repo_root/target/release/zmin}"
gix_bin="${GIX_BIN:-$(command -v gix 2>/dev/null || true)}"
commits="${ZMIN_BENCH_COMMITS:-90}"
files_per_commit="${ZMIN_BENCH_FILES_PER_COMMIT:-25}"
write_files="${ZMIN_BENCH_WRITE_FILES:-1800}"
dirty_files="${ZMIN_BENCH_DIRTY_FILES:-200}"
fetch_batch_files="${ZMIN_BENCH_FETCH_BATCH_FILES:-2400}"
push_batch_files="${ZMIN_BENCH_PUSH_BATCH_FILES:-2400}"
repeats="${ZMIN_BENCH_REPEATS:-10}"
seed="${ZMIN_BENCH_SEED:-1700000000}"
ops="${ZMIN_BENCH_OPS:-}"
out_dir="${ZMIN_BENCH_OUT_DIR:-}"
phase_trace_dir="${ZMIN_BENCH_PHASE_TRACE_DIR:-}"
ssh_trace_dir="${ZMIN_BENCH_SSH_TRACE_DIR:-}"
ssh_packet_trace_dir="${ZMIN_BENCH_SSH_PACKET_TRACE_DIR:-}"

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
    if [[ "${#selected_ops[@]}" -eq 0 ]] || ! op_in_list "$op" "${selected_ops[@]}"; then
      selected_ops+=("$op")
    fi
  done < <(printf '%s\n' "$ops" | tr ',;' '\n\n' | tr '[:space:]' '\n')
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
  local value="${1//\'/\'\\\'\'}"
  printf "'%s'" "$value"
}

if [[ "${#selected_ops[@]}" -gt 0 ]]; then
  printf 'selected_ops=%s\n' "$(selected_ops_label)" >&2
fi

if [[ -z "${ZMIN_BIN:-}" ]]; then
  cargo build --manifest-path "$repo_root/Cargo.toml" --release -p zmin-cli --bin zmin >/dev/null
fi
if [[ ! -x "$zmin_bin" ]]; then
  printf 'zmin release binary was not found or is not executable: %s\n' "$zmin_bin" >&2
  exit 1
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

if [[ -n "$out_dir" ]]; then
  mkdir -p "$out_dir"
fi

tmp_dir="$(mktemp -d)"
daemon_pid=""
cleanup() {
  if [[ -n "${daemon_pid:-}" ]]; then
    kill "$daemon_pid" >/dev/null 2>&1 || true
    wait "$daemon_pid" >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

out="$tmp_dir/bench.tsv"
validation_out="$tmp_dir/validation.tsv"
src="$tmp_dir/src"
remote="$tmp_dir/remote.git"
printf 'tool\top\treal\tuser\tsys\trss\texit\textra\n' >"$out"
printf 'check\tstatus\tdetails\n' >"$validation_out"

record_validation() {
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$validation_out"
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

measure_sh() {
  local tool="$1" op="$2" extra="$3" script="$4"
  local time_file="$tmp_dir/time-$tool-$op-$(date +%s%N).txt"
  local trace_file=""
  local trace_env=()
  local start_ns end_ns real_seconds
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
  start_ns="$(date +%s%N)"
  if [[ "${#trace_env[@]}" -gt 0 ]]; then
    env "${trace_env[@]}" /usr/bin/time -lp bash -lc "$script" >/dev/null 2>"$time_file"
  else
    /usr/bin/time -lp bash -lc "$script" >/dev/null 2>"$time_file"
  fi
  local status=$?
  end_ns="$(date +%s%N)"
  set -e
  real_seconds="$(python3 - "$start_ns" "$end_ns" <<'PY'
import sys

start = int(sys.argv[1])
end = int(sys.argv[2])
print(f"{(end - start) / 1_000_000_000:.6f}")
PY
  )"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$tool" \
    "$op" \
    "$real_seconds" \
    "$(time_field user "$time_file")" \
    "$(time_field sys "$time_file")" \
    "$(awk '/maximum resident set size/{print $1}' "$time_file" | tail -1)" \
    "$status" \
    "$extra" >>"$out"
}

time_field() {
  local field="$1" time_file="$2"
  awk -v field="$field" '{
    gsub(/\033\[[0-9;]*[A-Za-z]/, " ")
    gsub(/\r/, " ")
    for (idx = 1; idx < NF; idx++) {
      if ($idx == field) {
        print $(idx + 1)
      }
    }
  }' "$time_file" | tail -1
}

run_group() {
  local op="$1" extra="$2" group_seed="$3"
  shift 3
  local spec_file="$tmp_dir/spec-$op-$group_seed.tsv"
  printf '%s\n' "$@" >"$spec_file"
  while IFS=$'\t' read -r tool script; do
    measure_sh "$tool" "$op" "$extra" "$script"
  done < <(python3 - "$group_seed" "$spec_file" <<'PY'
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
  python3 - <<'PY'
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

"$git_bin" init -q -b main "$src"
configure_repo "$src"

for c in $(seq 1 "$commits"); do
  mkdir -p "$src/dir-$((c % 24))"
  for f in $(seq 1 "$files_per_commit"); do
    printf 'commit=%03d file=%03d payload=%04096d\n' "$c" "$f" 0 \
      >"$src/dir-$((c % 24))/file-$f.txt"
  done
  "$git_bin" -C "$src" add -A
  ts=$((1700000000 + c))
  GIT_AUTHOR_DATE="$ts +0000" GIT_COMMITTER_DATE="$ts +0000" \
    "$git_bin" -C "$src" commit -qm "commit $c"
done

"$git_bin" -C "$src" repack -adq
"$git_bin" -C "$src" fsck --strict >/dev/null
"$git_bin" -C "$src" rev-list --objects --all --no-object-names >"$tmp_dir/objects.txt"
object_count="$(wc -l <"$tmp_dir/objects.txt" | tr -d ' ')"
if any_benchmark_op_enabled status log rev-list merge-base; then
  validate_clean_git_zmin_outputs
fi
if any_benchmark_op_enabled pack-objects index-pack; then
  validate_pack_output
fi

for n in $(seq 1 "$repeats"); do
  if benchmark_op_enabled init; then
    specs=(
      $'git\t'"'$git_bin' init -q '$tmp_dir/git-init-$n'"
      $'zmin\t'"'$zmin_bin' init '$tmp_dir/zmin-init-$n'"
    )
    run_group init "$n" "$((seed + n))" "${specs[@]}"
  fi

  if benchmark_op_enabled status; then
    specs=(
      $'git\t'"cd '$src' && '$git_bin' status --porcelain=v1 --branch"
      $'zmin\t'"cd '$src' && '$zmin_bin' status --porcelain=v1 --branch"
    )
    if [[ -n "$gix_bin" ]]; then
      specs+=($'gix\t'"'$gix_bin' -r '$src' status --format simplified")
    fi
    run_group status "$n" "$((seed + 100 + n))" "${specs[@]}"
  fi

  if benchmark_op_enabled log; then
    specs=(
      $'git\t'"cd '$src' && '$git_bin' log --oneline --max-count '$commits'"
      $'zmin\t'"cd '$src' && '$zmin_bin' log --oneline --max-count '$commits'"
    )
    if [[ -n "$gix_bin" ]]; then
      specs+=($'gix\t'"'$gix_bin' -r '$src' log")
    fi
    run_group log "$n" "$((seed + 200 + n))" "${specs[@]}"
  fi

  if benchmark_op_enabled rev-list; then
    run_group rev-list "$n" "$((seed + 300 + n))" \
      $'git\t'"cd '$src' && '$git_bin' rev-list --objects --all" \
      $'zmin\t'"cd '$src' && '$zmin_bin' rev-list --objects --all"
  fi

  if benchmark_op_enabled merge-base; then
    specs=(
      $'git\t'"cd '$src' && '$git_bin' merge-base HEAD HEAD~$((commits / 2))"
      $'zmin\t'"cd '$src' && '$zmin_bin' merge-base HEAD HEAD~$((commits / 2))"
    )
    if [[ -n "$gix_bin" ]]; then
      specs+=($'gix\t'"'$gix_bin' -r '$src' merge-base HEAD HEAD~$((commits / 2))")
    fi
    run_group merge-base "$n" "$((seed + 400 + n))" "${specs[@]}"
  fi

  if benchmark_op_enabled pack-objects; then
    run_group pack-objects "$object_count objects" "$((seed + 500 + n))" \
      $'git\t'"cd '$src' && '$git_bin' pack-objects --stdout < '$tmp_dir/objects.txt' > '$tmp_dir/git-$n.pack'" \
      $'zmin\t'"cd '$src' && '$zmin_bin' pack-objects --stdout < '$tmp_dir/objects.txt' > '$tmp_dir/zmin-$n.pack'"
  elif benchmark_op_enabled index-pack; then
    "$git_bin" -C "$src" pack-objects --stdout <"$tmp_dir/objects.txt" >"$tmp_dir/git-$n.pack"
  fi

  if benchmark_op_enabled index-pack; then
    run_group index-pack "$n" "$((seed + 600 + n))" \
      $'git\t'"cd '$tmp_dir' && rm -rf git-index-$n && '$git_bin' init -q git-index-$n && '$git_bin' -C git-index-$n index-pack --stdin < '$tmp_dir/git-$n.pack'" \
      $'zmin\t'"cd '$tmp_dir' && rm -rf zmin-index-$n && '$git_bin' init -q zmin-index-$n && cd zmin-index-$n && '$zmin_bin' index-pack --stdin < '$tmp_dir/git-$n.pack'"
  fi
done

if any_benchmark_op_enabled add commit add-dirty commit-dirty; then
for n in $(seq 1 "$repeats"); do
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
      $'git\t'"cd '$git_repo' && '$git_bin' add -A" \
      $'zmin\t'"cd '$zmin_repo' && '$zmin_bin' add -A"
  elif any_benchmark_op_enabled commit add-dirty commit-dirty; then
    "$git_bin" -C "$git_repo" add -A
    "$zmin_bin" -C "$zmin_repo" add -A
  fi

  if benchmark_op_enabled commit; then
    run_group commit "$n/$write_files files" "$((seed + 800 + n))" \
      $'git\t'"cd '$git_repo' && GIT_AUTHOR_DATE='1700000000 +0000' GIT_COMMITTER_DATE='1700000000 +0000' '$git_bin' commit -qm initial" \
      $'zmin\t'"cd '$zmin_repo' && GIT_AUTHOR_DATE='1700000000 +0000' GIT_COMMITTER_DATE='1700000000 +0000' '$zmin_bin' commit -qm initial"
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
        $'git\t'"cd '$git_repo' && '$git_bin' add -A" \
        $'zmin\t'"cd '$zmin_repo' && '$zmin_bin' add -A"
    elif benchmark_op_enabled commit-dirty; then
      "$git_bin" -C "$git_repo" add -A
      "$zmin_bin" -C "$zmin_repo" add -A
    fi
    if benchmark_op_enabled commit-dirty; then
      run_group commit-dirty "$n/$dirty_files files" "$((seed + 1000 + n))" \
        $'git\t'"cd '$git_repo' && GIT_AUTHOR_DATE='1700000001 +0000' GIT_COMMITTER_DATE='1700000001 +0000' '$git_bin' commit -qm dirty" \
        $'zmin\t'"cd '$zmin_repo' && GIT_AUTHOR_DATE='1700000001 +0000' GIT_COMMITTER_DATE='1700000001 +0000' '$zmin_bin' commit -qm dirty"
      "$git_bin" -C "$zmin_repo" fsck --strict >/dev/null
      compare_trees "commit-dirty-$n" "$git_repo" "$zmin_repo" HEAD
    fi
  fi
done
fi

if any_benchmark_op_enabled clone clone-instant; then
for n in $(seq 1 "$repeats"); do
  if benchmark_op_enabled clone; then
    clone_specs=(
      $'git\t'"'$git_bin' clone -q '$src' '$tmp_dir/git-clone-$n'"
      $'zmin\t'"'$zmin_bin' clone -q '$src' '$tmp_dir/zmin-clone-$n'"
    )
    if [[ -n "$gix_bin" ]]; then
      clone_specs+=($'gix\t'"'$gix_bin' clone '$src' '$tmp_dir/gix-clone-$n'")
    fi
    run_group clone "$n/local" "$((seed + 1100 + n))" "${clone_specs[@]}"
    compare_refs "clone-$n" "$tmp_dir/git-clone-$n" "$tmp_dir/zmin-clone-$n" HEAD
    compare_trees "clone-$n-tree" "$tmp_dir/git-clone-$n" "$tmp_dir/zmin-clone-$n" HEAD
  fi

  if benchmark_op_enabled clone-instant; then
    run_group clone-instant "$n/local" "$((seed + 1150 + n))" \
      $'git\t'"'$git_bin' clone -q '$src' '$tmp_dir/git-clone-instant-$n'" \
      $'zmin\t'"'$zmin_bin' clone -q --instant '$src' '$tmp_dir/zmin-clone-instant-$n'"
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

for n in $(seq 1 "$repeats"); do
  if benchmark_op_enabled clone-instant-git-daemon; then
    run_group clone-instant-git-daemon "$n/git-daemon" "$((seed + 1160 + n))" \
      $'git\t'"'$git_bin' clone -q '$daemon_url' '$tmp_dir/git-daemon-instant-baseline-$n'" \
      $'zmin\t'"'$zmin_bin' clone -q --instant '$daemon_url' '$tmp_dir/zmin-daemon-instant-$n'"
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
      $'git\t'"$fake_ssh_env '$git_bin' clone -q '$ssh_url' '$tmp_dir/git-ssh-instant-baseline-$n'" \
      $'zmin\t'"$fake_ssh_env '$zmin_bin' clone -q --instant '$ssh_url' '$tmp_dir/zmin-ssh-instant-$n'"
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
  for n in $(seq 1 "$repeats"); do
    run_group push-noop "$n/remote" "$((seed + 1200 + n))" \
      $'git\t'"cd '$tmp_dir/git-push-base' && '$git_bin' push origin main" \
      $'zmin\t'"cd '$tmp_dir/zmin-push-base' && '$zmin_bin' push origin main"
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
  for n in $(seq 1 "$repeats"); do
    run_group push-incremental "$n/remote" "$((seed + 1300 + n))" \
      $'git\t'"cd '$tmp_dir/git-push-base' && '$git_bin' push origin HEAD:refs/heads/git-incremental-$n" \
      $'zmin\t'"cd '$tmp_dir/zmin-push-base' && '$zmin_bin' push origin HEAD:refs/heads/zmin-incremental-$n"
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
for n in $(seq 1 "$repeats"); do
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
    $'git\t'"cd '$tmp_dir/git-push-batch-$n' && '$git_bin' push origin HEAD:refs/heads/git-push-batch-$n" \
    $'zmin\t'"cd '$tmp_dir/zmin-push-batch-$n' && '$zmin_bin' push origin HEAD:refs/heads/zmin-push-batch-$n"
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
  for n in $(seq 1 "$repeats"); do
    run_group pull-noop "$n/remote" "$((seed + 1450 + n))" \
      $'git\t'"cd '$tmp_dir/git-pull-base' && '$git_bin' pull --ff-only" \
      $'zmin\t'"cd '$tmp_dir/zmin-pull-base' && '$zmin_bin' pull --ff-only"
  done
  compare_refs pull-noop "$tmp_dir/git-pull-base" "$tmp_dir/zmin-pull-base" HEAD
fi

if benchmark_op_enabled pull-incremental; then
  printf 'pull incremental\n' >"$pull_src/pull-incremental.txt"
  "$git_bin" -C "$pull_src" add -A
  GIT_AUTHOR_DATE='1700085000 +0000' GIT_COMMITTER_DATE='1700085000 +0000' \
    "$git_bin" -C "$pull_src" commit -qm pull-incremental
  "$git_bin" -C "$pull_src" push -q origin main
  for n in $(seq 1 "$repeats"); do
    cp -R "$tmp_dir/git-pull-base" "$tmp_dir/git-pull-incremental-$n"
    cp -R "$tmp_dir/zmin-pull-base" "$tmp_dir/zmin-pull-incremental-$n"
    run_group pull-incremental "$n/remote" "$((seed + 1475 + n))" \
      $'git\t'"cd '$tmp_dir/git-pull-incremental-$n' && '$git_bin' pull --ff-only" \
      $'zmin\t'"cd '$tmp_dir/zmin-pull-incremental-$n' && '$zmin_bin' pull --ff-only"
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

if any_benchmark_op_enabled fetch-noop fetch-incremental; then
"$git_bin" init -q --bare "$remote"
"$git_bin" -C "$src" remote add origin "$remote"
"$git_bin" -C "$src" push -q origin main
"$git_bin" clone -q "$remote" "$tmp_dir/git-fetch"
"$zmin_bin" clone -q "$remote" "$tmp_dir/zmin-fetch" >/dev/null
if [[ -n "$gix_bin" ]]; then
  "$git_bin" clone -q "$remote" "$tmp_dir/gix-fetch"
fi
if benchmark_op_enabled fetch-noop; then
  specs=(
    $'git\t'"cd '$tmp_dir/git-fetch' && '$git_bin' fetch origin"
    $'zmin\t'"cd '$tmp_dir/zmin-fetch' && '$zmin_bin' fetch origin"
  )
  if [[ -n "$gix_bin" ]]; then
    specs+=($'gix\t'"'$gix_bin' -r '$tmp_dir/gix-fetch' fetch -r origin")
  fi
  for n in $(seq 1 "$repeats"); do
    run_group fetch-noop "$n/remote" "$((seed + 1500 + n))" "${specs[@]}"
  done
  compare_refs fetch-noop "$tmp_dir/git-fetch" "$tmp_dir/zmin-fetch" refs/remotes/origin/main
fi

if benchmark_op_enabled fetch-incremental; then
  printf 'new\n' >"$src/new-file.txt"
  "$git_bin" -C "$src" add new-file.txt
  GIT_AUTHOR_DATE='1700099999 +0000' GIT_COMMITTER_DATE='1700099999 +0000' \
  "$git_bin" -C "$src" commit -qm new
  "$git_bin" -C "$src" push -q origin main
  for n in $(seq 1 "$repeats"); do
    cp -R "$tmp_dir/git-fetch" "$tmp_dir/git-fetch-incremental-$n"
    cp -R "$tmp_dir/zmin-fetch" "$tmp_dir/zmin-fetch-incremental-$n"
    specs=(
      $'git\t'"cd '$tmp_dir/git-fetch-incremental-$n' && '$git_bin' fetch origin"
      $'zmin\t'"cd '$tmp_dir/zmin-fetch-incremental-$n' && '$zmin_bin' fetch origin"
    )
    if [[ -n "$gix_bin" ]]; then
      cp -R "$tmp_dir/gix-fetch" "$tmp_dir/gix-fetch-incremental-$n"
      specs+=($'gix\t'"'$gix_bin' -r '$tmp_dir/gix-fetch-incremental-$n' fetch -r origin")
    fi
    run_group fetch-incremental "$n/remote" "$((seed + 1600 + n))" "${specs[@]}"
    compare_refs "fetch-incremental-$n" "$tmp_dir/git-fetch-incremental-$n" "$tmp_dir/zmin-fetch-incremental-$n" refs/remotes/origin/main
  done
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
if [[ -n "$gix_bin" ]]; then
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
for n in $(seq 1 "$repeats"); do
  cp -R "$tmp_dir/git-fetch-batch-base" "$tmp_dir/git-fetch-batch-$n"
  cp -R "$tmp_dir/zmin-fetch-batch-base" "$tmp_dir/zmin-fetch-batch-$n"
  specs=(
    $'git\t'"cd '$tmp_dir/git-fetch-batch-$n' && '$git_bin' fetch origin"
    $'zmin\t'"cd '$tmp_dir/zmin-fetch-batch-$n' && '$zmin_bin' fetch origin"
  )
  if [[ -n "$gix_bin" ]]; then
    cp -R "$tmp_dir/gix-fetch-batch-base" "$tmp_dir/gix-fetch-batch-$n"
    specs+=($'gix\t'"'$gix_bin' -r '$tmp_dir/gix-fetch-batch-$n' fetch -r origin")
  fi
  run_group fetch-batch "$n/$fetch_batch_files files" "$((seed + 1700 + n))" "${specs[@]}"
  "$git_bin" -C "$tmp_dir/zmin-fetch-batch-$n" fsck --strict >/dev/null
  compare_refs "fetch-batch-$n" "$tmp_dir/git-fetch-batch-$n" "$tmp_dir/zmin-fetch-batch-$n" refs/remotes/origin/main
done
fi

cat "$out"
cat "$validation_out"
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
  cp "$out" "$rows_path"
  cp "$validation_out" "$checks_path"
  python3 - "$rows_path" "$summary_path" "$comparison_path" <<'PY'
import csv
import statistics
import sys
from collections import defaultdict

rows_path, summary_path, comparison_path = sys.argv[1:4]

rows_by_op_tool = defaultdict(list)
rows_by_op_tool_extra = defaultdict(dict)
with open(rows_path, encoding="utf-8", newline="") as handle:
    reader = csv.DictReader(handle, delimiter="\t")
    for row in reader:
        tool = row.get("tool", "")
        if tool not in {"git", "zmin", "gix"}:
            continue
        try:
            seconds = float(row["real"])
        except (KeyError, TypeError, ValueError):
            continue
        op = row["op"]
        rows_by_op_tool[(op, tool)].append(seconds)
        rows_by_op_tool_extra[(op, tool)][row.get("extra", "")] = seconds


def rounded(value):
    return f"{value:.6f}"


def ratio(numerator, denominator):
    if denominator == 0:
        return ""
    return rounded(numerator / denominator)


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
    summary_rows.append(
        {
            "op": op,
            "tool": tool,
            "runs": str(len(values)),
            "mean_seconds": rounded(statistics.mean(values)),
            "median_seconds": rounded(statistics.median(values)),
            "min_seconds": rounded(values[0]),
            "max_seconds": rounded(values[-1]),
        }
    )

with open(summary_path, "w", encoding="utf-8", newline="") as handle:
    fieldnames = [
        "op",
        "tool",
        "runs",
        "mean_seconds",
        "median_seconds",
        "min_seconds",
        "max_seconds",
    ]
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    writer.writerows(summary_rows)

ops = sorted({op for op, _ in rows_by_op_tool})
comparison_rows = []
for op in ops:
    git = sorted(rows_by_op_tool.get((op, "git"), []))
    zmin = sorted(rows_by_op_tool.get((op, "zmin"), []))
    gix = sorted(rows_by_op_tool.get((op, "gix"), []))
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
        }
    )

with open(comparison_path, "w", encoding="utf-8", newline="") as handle:
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
    ]
    writer = csv.DictWriter(handle, fieldnames=fieldnames)
    writer.writeheader()
    writer.writerows(comparison_rows)
PY
  printf 'rows=%s\n' "$rows_path" >&2
  printf 'checks=%s\n' "$checks_path" >&2
  printf 'summary=%s\n' "$summary_path" >&2
  printf 'comparison=%s\n' "$comparison_path" >&2
fi
