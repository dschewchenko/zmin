#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
git_bin="${GIT_BIN:-$(command -v git)}"
zmin_bin="${ZMIN_BIN:-$repo_root/target/release/zmin}"
commits="${ZMIN_HTTP_BENCH_COMMITS:-40}"
files_per_commit="${ZMIN_HTTP_BENCH_FILES_PER_COMMIT:-20}"
batch_files="${ZMIN_HTTP_BENCH_BATCH_FILES:-800}"
repeats="${ZMIN_HTTP_BENCH_REPEATS:-3}"
seed="${ZMIN_HTTP_BENCH_SEED:-1800000000}"
out_dir="${ZMIN_HTTP_BENCH_OUT_DIR:-$repo_root/target/bench/git-http-performance-$(date -u +%Y%m%dT%H%M%SZ)}"

if [[ ! -x "$zmin_bin" ]]; then
  cargo build --manifest-path "$repo_root/Cargo.toml" --release -p zmin-cli --bin zmin >/dev/null
fi

mkdir -p "$out_dir"
tmp_dir="$(mktemp -d)"
server_pid=""
cleanup() {
  if [[ -n "$server_pid" ]]; then
    kill "$server_pid" >/dev/null 2>&1 || true
    wait "$server_pid" >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

rows="$out_dir/bench.tsv"
checks="$out_dir/checks.tsv"
summary="$out_dir/summary.tsv"
comparison="$out_dir/comparison.tsv"
server_log="$out_dir/http-server.log"
src="$tmp_dir/src"
repos="$tmp_dir/repos"
server_root="$tmp_dir/http-root"
url=""

printf 'tool\top\treal\tuser\tsys\trss\texit\textra\n' >"$rows"
printf 'check\tstatus\tdetails\n' >"$checks"

record_validation() {
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$checks"
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

measure_sh() {
  local tool="$1" op="$2" extra="$3" script="$4"
  local time_file="$tmp_dir/time-$tool-$op-$(date +%s%N).txt"
  set +e
  /usr/bin/time -lp bash -lc "$script" >/dev/null 2>"$time_file"
  local status=$?
  set -e
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$tool" \
    "$op" \
    "$(time_field real "$time_file")" \
    "$(time_field user "$time_file")" \
    "$(time_field sys "$time_file")" \
    "$(awk '/maximum resident set size/{print $1}' "$time_file" | tail -1)" \
    "$status" \
    "$extra" >>"$rows"
  if [[ "$status" -ne 0 ]]; then
    cat "$time_file" >&2
    return "$status"
  fi
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

configure_repo() {
  local repo="$1"
  "$git_bin" -C "$repo" config user.name Bench
  "$git_bin" -C "$repo" config user.email bench@example.test
  "$git_bin" -C "$repo" config commit.gpgsign false
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

check_config_value() {
  local name="$1" repo="$2" key="$3" expected="$4"
  local actual
  actual="$("$git_bin" -C "$repo" config --get "$key")"
  if [[ "$actual" == "$expected" ]]; then
    record_validation "$name" ok "$key=$actual"
  else
    record_validation "$name" fail "$key: $actual != $expected"
    exit 1
  fi
}

make_fixture_file() {
  local path="$1" label="$2" index="$3"
  mkdir -p "$(dirname "$path")"
  printf '%s=%05d\npayload=%04096d\n' "$label" "$index" 0 >"$path"
}

build_source_repo() {
  "$git_bin" init -q -b main "$src"
  configure_repo "$src"
  for c in $(seq 1 "$commits"); do
    for f in $(seq 1 "$files_per_commit"); do
      make_fixture_file "$src/dir-$((c % 24))/file-$f.txt" "commit-$c-file" "$f"
    done
    "$git_bin" -C "$src" add -A
    ts=$((1800000000 + c))
    GIT_AUTHOR_DATE="$ts +0000" GIT_COMMITTER_DATE="$ts +0000" \
      "$git_bin" -C "$src" commit -qm "commit $c"
  done
  "$git_bin" -C "$src" repack -adq
  "$git_bin" -C "$src" fsck --strict >/dev/null
  mkdir -p "$repos"
  "$git_bin" clone -q --bare "$src" "$repos/remote.git"
}

start_http_backend() {
  mkdir -p "$server_root/cgi-bin"
  cat >"$server_root/cgi-bin/git-http-backend" <<SCRIPT
#!/usr/bin/env bash
export GIT_PROJECT_ROOT="$repos"
export GIT_HTTP_EXPORT_ALL=1
exec "$git_bin" http-backend
SCRIPT
  chmod +x "$server_root/cgi-bin/git-http-backend"
  local port
  port="$(python3 - <<'PY'
import socket

with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
  )"
  (cd "$server_root" && python3 -m http.server --cgi "$port" --bind 127.0.0.1) \
    >"$server_log" 2>&1 &
  server_pid="$!"
  url="http://127.0.0.1:$port/cgi-bin/git-http-backend/remote.git"
  for _ in $(seq 1 100); do
    if "$git_bin" ls-remote "$url" HEAD >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.1
  done
  cat "$server_log" >&2 || true
  echo "HTTP backend did not become ready at $url" >&2
  exit 1
}

write_summary() {
  python3 - "$rows" "$summary" "$comparison" <<'PY'
import csv
import os
import statistics
import sys
from collections import defaultdict

rows_path, summary_path, comparison_path = sys.argv[1:4]
groups = defaultdict(list)
with open(rows_path, newline="", encoding="utf-8") as handle:
    reader = csv.DictReader(handle, delimiter="\t")
    for row in reader:
        if row["exit"] != "0":
            continue
        groups[(row["op"], row["tool"])].append(float(row["real"]))

with open(summary_path, "w", newline="", encoding="utf-8") as handle:
    writer = csv.writer(handle, delimiter="\t")
    writer.writerow(["op", "tool", "runs", "mean_seconds", "median_seconds", "min_seconds", "max_seconds"])
    for (op, tool), values in sorted(groups.items()):
        writer.writerow([
            op,
            tool,
            len(values),
            f"{statistics.mean(values):.6f}",
            f"{statistics.median(values):.6f}",
            f"{min(values):.6f}",
            f"{max(values):.6f}",
        ])

with open(comparison_path, "w", newline="", encoding="utf-8") as handle:
    writer = csv.writer(handle, delimiter="\t")
    writer.writerow([
        "op",
        "runs",
        "git_mean_seconds",
        "zmin_mean_seconds",
        "zmin_vs_git_mean_ratio",
        "git_median_seconds",
        "zmin_median_seconds",
        "zmin_vs_git_median_ratio",
    ])
    comparison_rows = []
    for op in sorted({op for op, _ in groups}):
        git = groups.get((op, "git"))
        zmin = groups.get((op, "zmin"))
        if not git or not zmin:
            continue
        git_mean = statistics.mean(git)
        zmin_mean = statistics.mean(zmin)
        git_median = statistics.median(git)
        zmin_median = statistics.median(zmin)
        comparison_row = [
            op,
            min(len(git), len(zmin)),
            f"{git_mean:.6f}",
            f"{zmin_mean:.6f}",
            f"{zmin_mean / git_mean:.6f}",
            f"{git_median:.6f}",
            f"{zmin_median:.6f}",
            f"{zmin_median / git_median:.6f}",
        ]
        comparison_rows.append(dict(zip([
            "op",
            "runs",
            "git_mean_seconds",
            "zmin_mean_seconds",
            "zmin_vs_git_mean_ratio",
            "git_median_seconds",
            "zmin_median_seconds",
            "zmin_vs_git_median_ratio",
        ], comparison_row)))
        writer.writerow(comparison_row)


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
    max_ratio_from_env("ZMIN_HTTP_BENCH_MAX_ZMIN_VS_GIT_MEAN_RATIO"),
    "Zmin/Git mean",
)
assert_max_ratio(
    "zmin_vs_git_median_ratio",
    max_ratio_from_env("ZMIN_HTTP_BENCH_MAX_ZMIN_VS_GIT_MEDIAN_RATIO"),
    "Zmin/Git median",
)
PY
}

build_source_repo
start_http_backend

for n in $(seq 1 "$repeats"); do
  run_group clone-http "$n/smart-http" "$((seed + n))" \
    $'git\t'"'$git_bin' clone -q '$url' '$tmp_dir/git-http-clone-$n'" \
    $'zmin\t'"'$zmin_bin' clone -q '$url' '$tmp_dir/zmin-http-clone-$n'"
  "$git_bin" -C "$tmp_dir/zmin-http-clone-$n" fsck --strict >/dev/null
  compare_refs "clone-http-$n" "$tmp_dir/git-http-clone-$n" "$tmp_dir/zmin-http-clone-$n" HEAD
done

for n in $(seq 1 "$repeats"); do
  run_group clone-http-instant "$n/smart-http" "$((seed + 50 + n))" \
    $'git\t'"'$git_bin' clone -q '$url' '$tmp_dir/git-http-instant-baseline-$n'" \
    $'zmin\t'"'$zmin_bin' clone -q --instant '$url' '$tmp_dir/zmin-http-instant-$n'"
  "$git_bin" -C "$tmp_dir/zmin-http-instant-$n" fsck --strict >/dev/null
  compare_refs "clone-http-instant-$n" \
    "$tmp_dir/git-http-instant-baseline-$n" \
    "$tmp_dir/zmin-http-instant-$n" \
    HEAD
  compare_refs "clone-http-instant-$n-tree" \
    "$tmp_dir/git-http-instant-baseline-$n" \
    "$tmp_dir/zmin-http-instant-$n" \
    'HEAD^{tree}'
  check_config_value "clone-http-instant-$n-marker" \
    "$tmp_dir/zmin-http-instant-$n" \
    zmin.worktreeFirst \
    true
done

"$git_bin" clone -q "$url" "$tmp_dir/git-fetch-base"
"$zmin_bin" clone -q "$url" "$tmp_dir/zmin-fetch-base" >/dev/null
for n in $(seq 1 "$repeats"); do
  run_group fetch-http-noop "$n/smart-http" "$((seed + 100 + n))" \
    $'git\t'"cd '$tmp_dir/git-fetch-base' && '$git_bin' fetch origin" \
    $'zmin\t'"cd '$tmp_dir/zmin-fetch-base' && '$zmin_bin' fetch origin"
done
compare_refs fetch-http-noop "$tmp_dir/git-fetch-base" "$tmp_dir/zmin-fetch-base" refs/remotes/origin/main

printf 'incremental\n' >"$src/incremental.txt"
"$git_bin" -C "$src" add incremental.txt
GIT_AUTHOR_DATE='1800100000 +0000' GIT_COMMITTER_DATE='1800100000 +0000' \
  "$git_bin" -C "$src" commit -qm incremental
"$git_bin" -C "$src" push -q "$repos/remote.git" main

for n in $(seq 1 "$repeats"); do
  cp -R "$tmp_dir/git-fetch-base" "$tmp_dir/git-fetch-incremental-$n"
  cp -R "$tmp_dir/zmin-fetch-base" "$tmp_dir/zmin-fetch-incremental-$n"
  run_group fetch-http-incremental "$n/smart-http" "$((seed + 200 + n))" \
    $'git\t'"cd '$tmp_dir/git-fetch-incremental-$n' && '$git_bin' fetch origin" \
    $'zmin\t'"cd '$tmp_dir/zmin-fetch-incremental-$n' && '$zmin_bin' fetch origin"
  compare_refs "fetch-http-incremental-$n" \
    "$tmp_dir/git-fetch-incremental-$n" \
    "$tmp_dir/zmin-fetch-incremental-$n" \
    refs/remotes/origin/main
done

mkdir -p "$src/batch"
for i in $(seq 1 "$batch_files"); do
  make_fixture_file "$src/batch/file-$i.txt" batch "$i"
done
"$git_bin" -C "$src" add -A
GIT_AUTHOR_DATE='1800100001 +0000' GIT_COMMITTER_DATE='1800100001 +0000' \
  "$git_bin" -C "$src" commit -qm batch
"$git_bin" -C "$src" push -q "$repos/remote.git" main

for n in $(seq 1 "$repeats"); do
  cp -R "$tmp_dir/git-fetch-base" "$tmp_dir/git-fetch-batch-$n"
  cp -R "$tmp_dir/zmin-fetch-base" "$tmp_dir/zmin-fetch-batch-$n"
  run_group fetch-http-batch "$n/$batch_files files" "$((seed + 300 + n))" \
    $'git\t'"cd '$tmp_dir/git-fetch-batch-$n' && '$git_bin' fetch origin" \
    $'zmin\t'"cd '$tmp_dir/zmin-fetch-batch-$n' && '$zmin_bin' fetch origin"
  "$git_bin" -C "$tmp_dir/zmin-fetch-batch-$n" fsck --strict >/dev/null
  compare_refs "fetch-http-batch-$n" \
    "$tmp_dir/git-fetch-batch-$n" \
    "$tmp_dir/zmin-fetch-batch-$n" \
    refs/remotes/origin/main
done

write_summary

printf 'rows=%s\n' "$rows"
printf 'checks=%s\n' "$checks"
printf 'summary=%s\n' "$summary"
printf 'comparison=%s\n' "$comparison"
printf 'server_log=%s\n' "$server_log"
cat "$summary"
cat "$comparison"
