#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
git_bin="${GIT_BIN:-$(command -v git)}"
skron_bin="${SKRON_BIN:-$repo_root/target/release/skron-git}"
gix_bin="${GIX_BIN:-$(command -v gix 2>/dev/null || true)}"
commits="${SKRON_BENCH_COMMITS:-90}"
files_per_commit="${SKRON_BENCH_FILES_PER_COMMIT:-25}"
write_files="${SKRON_BENCH_WRITE_FILES:-1800}"
dirty_files="${SKRON_BENCH_DIRTY_FILES:-200}"
fetch_batch_files="${SKRON_BENCH_FETCH_BATCH_FILES:-2400}"
push_batch_files="${SKRON_BENCH_PUSH_BATCH_FILES:-2400}"
repeats="${SKRON_BENCH_REPEATS:-10}"
seed="${SKRON_BENCH_SEED:-1700000000}"

if [[ ! -x "$skron_bin" ]]; then
  cargo build --manifest-path "$repo_root/Cargo.toml" --release -p skron-cli --bin skron-git >/dev/null
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

out="$tmp_dir/bench.tsv"
validation_out="$tmp_dir/validation.tsv"
src="$tmp_dir/src"
remote="$tmp_dir/remote.git"
printf 'tool\top\treal\tuser\tsys\trss\texit\textra\n' >"$out"
printf 'check\tstatus\tdetails\n' >"$validation_out"

record_validation() {
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$validation_out"
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

validate_clean_git_skron_outputs() {
  "$git_bin" -C "$src" status --porcelain=v1 --branch >"$tmp_dir/git-status.txt"
  "$skron_bin" -C "$src" status --porcelain=v1 --branch >"$tmp_dir/skron-status.txt"
  compare_files status "$tmp_dir/git-status.txt" "$tmp_dir/skron-status.txt"

  "$git_bin" -C "$src" log --oneline --max-count "$commits" >"$tmp_dir/git-log.txt"
  "$skron_bin" -C "$src" log --oneline --max-count "$commits" >"$tmp_dir/skron-log.txt"
  compare_files log "$tmp_dir/git-log.txt" "$tmp_dir/skron-log.txt"

  "$git_bin" -C "$src" rev-list --objects --all >"$tmp_dir/git-rev-list.txt"
  "$skron_bin" -C "$src" rev-list --objects --all >"$tmp_dir/skron-rev-list.txt"
  compare_files rev-list "$tmp_dir/git-rev-list.txt" "$tmp_dir/skron-rev-list.txt"

  "$git_bin" -C "$src" merge-base HEAD "HEAD~$((commits / 2))" >"$tmp_dir/git-merge-base.txt"
  "$skron_bin" -C "$src" merge-base HEAD "HEAD~$((commits / 2))" >"$tmp_dir/skron-merge-base.txt"
  compare_files merge-base "$tmp_dir/git-merge-base.txt" "$tmp_dir/skron-merge-base.txt"
}

validate_pack_output() {
  "$git_bin" -C "$src" pack-objects --stdout <"$tmp_dir/objects.txt" >"$tmp_dir/validate-git.pack"
  "$skron_bin" -C "$src" pack-objects --stdout <"$tmp_dir/objects.txt" >"$tmp_dir/validate-skron.pack"
  "$git_bin" init -q "$tmp_dir/validate-git-index"
  "$git_bin" init -q "$tmp_dir/validate-skron-index"
  "$git_bin" -C "$tmp_dir/validate-git-index" index-pack --stdin <"$tmp_dir/validate-git.pack" >/dev/null
  "$git_bin" -C "$tmp_dir/validate-skron-index" index-pack --stdin <"$tmp_dir/validate-skron.pack" >/dev/null
  record_validation pack-objects ok "git_index_pack_accepts_git_and_skron_packs"
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
validate_clean_git_skron_outputs
validate_pack_output

for n in $(seq 1 "$repeats"); do
  specs=(
    $'git\t'"'$git_bin' init -q '$tmp_dir/git-init-$n'"
    $'skron\t'"'$skron_bin' init '$tmp_dir/skron-init-$n'"
  )
  run_group init "$n" "$((seed + n))" "${specs[@]}"

  specs=(
    $'git\t'"cd '$src' && '$git_bin' status --porcelain=v1 --branch"
    $'skron\t'"cd '$src' && '$skron_bin' status --porcelain=v1 --branch"
  )
  if [[ -n "$gix_bin" ]]; then
    specs+=($'gix\t'"'$gix_bin' -r '$src' status --format simplified")
  fi
  run_group status "$n" "$((seed + 100 + n))" "${specs[@]}"

  specs=(
    $'git\t'"cd '$src' && '$git_bin' log --oneline --max-count '$commits'"
    $'skron\t'"cd '$src' && '$skron_bin' log --oneline --max-count '$commits'"
  )
  if [[ -n "$gix_bin" ]]; then
    specs+=($'gix\t'"'$gix_bin' -r '$src' log")
  fi
  run_group log "$n" "$((seed + 200 + n))" "${specs[@]}"

  run_group rev-list "$n" "$((seed + 300 + n))" \
    $'git\t'"cd '$src' && '$git_bin' rev-list --objects --all" \
    $'skron\t'"cd '$src' && '$skron_bin' rev-list --objects --all"

  specs=(
    $'git\t'"cd '$src' && '$git_bin' merge-base HEAD HEAD~$((commits / 2))"
    $'skron\t'"cd '$src' && '$skron_bin' merge-base HEAD HEAD~$((commits / 2))"
  )
  if [[ -n "$gix_bin" ]]; then
    specs+=($'gix\t'"'$gix_bin' -r '$src' merge-base HEAD HEAD~$((commits / 2))")
  fi
  run_group merge-base "$n" "$((seed + 400 + n))" "${specs[@]}"

  run_group pack-objects "$object_count objects" "$((seed + 500 + n))" \
    $'git\t'"cd '$src' && '$git_bin' pack-objects --stdout < '$tmp_dir/objects.txt' > '$tmp_dir/git-$n.pack'" \
    $'skron\t'"cd '$src' && '$skron_bin' pack-objects --stdout < '$tmp_dir/objects.txt' > '$tmp_dir/skron-$n.pack'"

  run_group index-pack "$n" "$((seed + 600 + n))" \
    $'git\t'"cd '$tmp_dir' && rm -rf git-index-$n && '$git_bin' init -q git-index-$n && '$git_bin' -C git-index-$n index-pack --stdin < '$tmp_dir/git-$n.pack'" \
    $'skron\t'"cd '$tmp_dir' && rm -rf skron-index-$n && '$git_bin' init -q skron-index-$n && cd skron-index-$n && '$skron_bin' index-pack --stdin < '$tmp_dir/git-$n.pack'"
done

for n in $(seq 1 "$repeats"); do
  git_repo="$tmp_dir/git-write-$n"
  skron_repo="$tmp_dir/skron-write-$n"
  "$git_bin" init -q -b main "$git_repo"
  "$skron_bin" init "$skron_repo" >/dev/null
  configure_repo "$git_repo"
  configure_repo "$skron_repo"
  make_files "$git_repo" "$write_files" file
  make_files "$skron_repo" "$write_files" file

  run_group add "$n/$write_files files" "$((seed + 700 + n))" \
    $'git\t'"cd '$git_repo' && '$git_bin' add -A" \
    $'skron\t'"cd '$skron_repo' && '$skron_bin' add -A"
  run_group commit "$n/$write_files files" "$((seed + 800 + n))" \
    $'git\t'"cd '$git_repo' && GIT_AUTHOR_DATE='1700000000 +0000' GIT_COMMITTER_DATE='1700000000 +0000' '$git_bin' commit -qm initial" \
    $'skron\t'"cd '$skron_repo' && GIT_AUTHOR_DATE='1700000000 +0000' GIT_COMMITTER_DATE='1700000000 +0000' '$skron_bin' commit -qm initial"
  "$git_bin" -C "$skron_repo" fsck --strict >/dev/null
  compare_trees "commit-$n" "$git_repo" "$skron_repo" HEAD

  for i in $(seq 1 "$dirty_files"); do
    printf 'changed %05d\n' "$i" >>"$git_repo/dir-$((i % 32))/file-$i.txt"
    printf 'changed %05d\n' "$i" >>"$skron_repo/dir-$((i % 32))/file-$i.txt"
  done
  run_group add-dirty "$n/$dirty_files files" "$((seed + 900 + n))" \
    $'git\t'"cd '$git_repo' && '$git_bin' add -A" \
    $'skron\t'"cd '$skron_repo' && '$skron_bin' add -A"
  run_group commit-dirty "$n/$dirty_files files" "$((seed + 1000 + n))" \
    $'git\t'"cd '$git_repo' && GIT_AUTHOR_DATE='1700000001 +0000' GIT_COMMITTER_DATE='1700000001 +0000' '$git_bin' commit -qm dirty" \
    $'skron\t'"cd '$skron_repo' && GIT_AUTHOR_DATE='1700000001 +0000' GIT_COMMITTER_DATE='1700000001 +0000' '$skron_bin' commit -qm dirty"
  "$git_bin" -C "$skron_repo" fsck --strict >/dev/null
  compare_trees "commit-dirty-$n" "$git_repo" "$skron_repo" HEAD
done

for n in $(seq 1 "$repeats"); do
  clone_specs=(
    $'git\t'"'$git_bin' clone -q '$src' '$tmp_dir/git-clone-$n'"
    $'skron\t'"'$skron_bin' clone -q '$src' '$tmp_dir/skron-clone-$n'"
  )
  if [[ -n "$gix_bin" ]]; then
    clone_specs+=($'gix\t'"'$gix_bin' clone '$src' '$tmp_dir/gix-clone-$n'")
  fi
  run_group clone "$n/local" "$((seed + 1100 + n))" "${clone_specs[@]}"
  compare_refs "clone-$n" "$tmp_dir/git-clone-$n" "$tmp_dir/skron-clone-$n" HEAD
done

push_remote="$tmp_dir/push-remote.git"
"$git_bin" init -q --bare "$push_remote"
"$git_bin" clone -q "$src" "$tmp_dir/git-push-base"
"$skron_bin" clone -q "$src" "$tmp_dir/skron-push-base" >/dev/null
"$git_bin" -C "$tmp_dir/git-push-base" remote remove origin
"$git_bin" -C "$tmp_dir/skron-push-base" remote remove origin
"$git_bin" -C "$tmp_dir/git-push-base" remote add origin "$push_remote"
"$git_bin" -C "$tmp_dir/skron-push-base" remote add origin "$push_remote"
"$git_bin" -C "$tmp_dir/git-push-base" push -q origin main
for n in $(seq 1 "$repeats"); do
  run_group push-noop "$n/remote" "$((seed + 1200 + n))" \
    $'git\t'"cd '$tmp_dir/git-push-base' && '$git_bin' push origin main" \
    $'skron\t'"cd '$tmp_dir/skron-push-base' && '$skron_bin' push origin main"
done

printf 'incremental\n' >"$tmp_dir/git-push-base/incremental.txt"
printf 'incremental\n' >"$tmp_dir/skron-push-base/incremental.txt"
"$git_bin" -C "$tmp_dir/git-push-base" add -A
"$git_bin" -C "$tmp_dir/skron-push-base" add -A
GIT_AUTHOR_DATE='1700080000 +0000' GIT_COMMITTER_DATE='1700080000 +0000' \
  "$git_bin" -C "$tmp_dir/git-push-base" commit -qm incremental
GIT_AUTHOR_DATE='1700080000 +0000' GIT_COMMITTER_DATE='1700080000 +0000' \
  "$skron_bin" -C "$tmp_dir/skron-push-base" commit -qm incremental >/dev/null
compare_trees push-incremental-prep "$tmp_dir/git-push-base" "$tmp_dir/skron-push-base" HEAD
for n in $(seq 1 "$repeats"); do
  run_group push-incremental "$n/remote" "$((seed + 1300 + n))" \
    $'git\t'"cd '$tmp_dir/git-push-base' && '$git_bin' push origin HEAD:refs/heads/git-incremental-$n" \
    $'skron\t'"cd '$tmp_dir/skron-push-base' && '$skron_bin' push origin HEAD:refs/heads/skron-incremental-$n"
  "$git_bin" --git-dir "$push_remote" rev-parse "refs/heads/git-incremental-$n" >/dev/null
  "$git_bin" --git-dir "$push_remote" rev-parse "refs/heads/skron-incremental-$n" >/dev/null
done
record_validation push-incremental ok refs_present

push_batch_remote="$tmp_dir/push-batch-remote.git"
"$git_bin" init -q --bare "$push_batch_remote"
"$git_bin" clone -q "$src" "$tmp_dir/git-push-batch-base"
"$skron_bin" clone -q "$src" "$tmp_dir/skron-push-batch-base" >/dev/null
"$git_bin" -C "$tmp_dir/git-push-batch-base" remote remove origin
"$git_bin" -C "$tmp_dir/skron-push-batch-base" remote remove origin
"$git_bin" -C "$tmp_dir/git-push-batch-base" remote add origin "$push_batch_remote"
"$git_bin" -C "$tmp_dir/skron-push-batch-base" remote add origin "$push_batch_remote"
"$git_bin" -C "$tmp_dir/git-push-batch-base" push -q origin main
for n in $(seq 1 "$repeats"); do
  cp -R "$tmp_dir/git-push-batch-base" "$tmp_dir/git-push-batch-$n"
  cp -R "$tmp_dir/skron-push-batch-base" "$tmp_dir/skron-push-batch-$n"
  mkdir -p "$tmp_dir/git-push-batch-$n/push-batch" "$tmp_dir/skron-push-batch-$n/push-batch"
  for i in $(seq 1 "$push_batch_files"); do
    printf 'push batch %04d %04096d\n' "$i" 0 >"$tmp_dir/git-push-batch-$n/push-batch/file-$i.txt"
    printf 'push batch %04d %04096d\n' "$i" 0 >"$tmp_dir/skron-push-batch-$n/push-batch/file-$i.txt"
  done
  "$git_bin" -C "$tmp_dir/git-push-batch-$n" add -A
  "$git_bin" -C "$tmp_dir/skron-push-batch-$n" add -A
  ts=$((1700081000 + n))
  GIT_AUTHOR_DATE="$ts +0000" GIT_COMMITTER_DATE="$ts +0000" \
    "$git_bin" -C "$tmp_dir/git-push-batch-$n" commit -qm push-batch
  GIT_AUTHOR_DATE="$ts +0000" GIT_COMMITTER_DATE="$ts +0000" \
    "$skron_bin" -C "$tmp_dir/skron-push-batch-$n" commit -qm push-batch >/dev/null
  compare_trees "push-batch-prep-$n" "$tmp_dir/git-push-batch-$n" "$tmp_dir/skron-push-batch-$n" HEAD
  run_group push-batch "$n/$push_batch_files files" "$((seed + 1400 + n))" \
    $'git\t'"cd '$tmp_dir/git-push-batch-$n' && '$git_bin' push origin HEAD:refs/heads/git-push-batch-$n" \
    $'skron\t'"cd '$tmp_dir/skron-push-batch-$n' && '$skron_bin' push origin HEAD:refs/heads/skron-push-batch-$n"
done
record_validation push-batch ok refs_pushed

"$git_bin" init -q --bare "$remote"
"$git_bin" -C "$src" remote add origin "$remote"
"$git_bin" -C "$src" push -q origin main
"$git_bin" clone -q "$remote" "$tmp_dir/git-fetch"
"$skron_bin" clone -q "$remote" "$tmp_dir/skron-fetch" >/dev/null
if [[ -n "$gix_bin" ]]; then
  "$git_bin" clone -q "$remote" "$tmp_dir/gix-fetch"
fi
specs=(
  $'git\t'"cd '$tmp_dir/git-fetch' && '$git_bin' fetch origin"
  $'skron\t'"cd '$tmp_dir/skron-fetch' && '$skron_bin' fetch origin"
)
if [[ -n "$gix_bin" ]]; then
  specs+=($'gix\t'"'$gix_bin' -r '$tmp_dir/gix-fetch' fetch -r origin")
fi
for n in $(seq 1 "$repeats"); do
  run_group fetch-noop "$n/remote" "$((seed + 1500 + n))" "${specs[@]}"
done
compare_refs fetch-noop "$tmp_dir/git-fetch" "$tmp_dir/skron-fetch" refs/remotes/origin/main

printf 'new\n' >"$src/new-file.txt"
"$git_bin" -C "$src" add new-file.txt
GIT_AUTHOR_DATE='1700099999 +0000' GIT_COMMITTER_DATE='1700099999 +0000' \
"$git_bin" -C "$src" commit -qm new
"$git_bin" -C "$src" push -q origin main
for n in $(seq 1 "$repeats"); do
  cp -R "$tmp_dir/git-fetch" "$tmp_dir/git-fetch-incremental-$n"
  cp -R "$tmp_dir/skron-fetch" "$tmp_dir/skron-fetch-incremental-$n"
  specs=(
    $'git\t'"cd '$tmp_dir/git-fetch-incremental-$n' && '$git_bin' fetch origin"
    $'skron\t'"cd '$tmp_dir/skron-fetch-incremental-$n' && '$skron_bin' fetch origin"
  )
  if [[ -n "$gix_bin" ]]; then
    cp -R "$tmp_dir/gix-fetch" "$tmp_dir/gix-fetch-incremental-$n"
    specs+=($'gix\t'"'$gix_bin' -r '$tmp_dir/gix-fetch-incremental-$n' fetch -r origin")
  fi
  run_group fetch-incremental "$n/remote" "$((seed + 1600 + n))" "${specs[@]}"
  compare_refs "fetch-incremental-$n" "$tmp_dir/git-fetch-incremental-$n" "$tmp_dir/skron-fetch-incremental-$n" refs/remotes/origin/main
done

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
"$skron_bin" clone -q "$batch_remote" "$tmp_dir/skron-fetch-batch-base" >/dev/null
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
  cp -R "$tmp_dir/skron-fetch-batch-base" "$tmp_dir/skron-fetch-batch-$n"
  specs=(
    $'git\t'"cd '$tmp_dir/git-fetch-batch-$n' && '$git_bin' fetch origin"
    $'skron\t'"cd '$tmp_dir/skron-fetch-batch-$n' && '$skron_bin' fetch origin"
  )
  if [[ -n "$gix_bin" ]]; then
    cp -R "$tmp_dir/gix-fetch-batch-base" "$tmp_dir/gix-fetch-batch-$n"
    specs+=($'gix\t'"'$gix_bin' -r '$tmp_dir/gix-fetch-batch-$n' fetch -r origin")
  fi
  run_group fetch-batch "$n/$fetch_batch_files files" "$((seed + 1700 + n))" "${specs[@]}"
  "$git_bin" -C "$tmp_dir/skron-fetch-batch-$n" fsck --strict >/dev/null
  compare_refs "fetch-batch-$n" "$tmp_dir/git-fetch-batch-$n" "$tmp_dir/skron-fetch-batch-$n" refs/remotes/origin/main
done

cat "$out"
cat "$validation_out"
if [[ -f "$tmp_dir/git-1.pack" ]]; then
  printf 'pack_bytes\tgit\t%s\n' "$(wc -c <"$tmp_dir/git-1.pack" | tr -d ' ')"
fi
if [[ -f "$tmp_dir/skron-1.pack" ]]; then
  printf 'pack_bytes\tskron\t%s\n' "$(wc -c <"$tmp_dir/skron-1.pack" | tr -d ' ')"
fi
