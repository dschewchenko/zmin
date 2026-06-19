#!/usr/bin/env bash
set -euo pipefail

profile="${ZMIN_STRESS_PROFILE:-small}"

case "$profile" in
  small)
    files="${ZMIN_STRESS_FILES:-120}"
    big_kib="${ZMIN_STRESS_BIG_KIB:-512}"
    ;;
  medium)
    files="${ZMIN_STRESS_FILES:-800}"
    big_kib="${ZMIN_STRESS_BIG_KIB:-4096}"
    ;;
  large)
    files="${ZMIN_STRESS_FILES:-3000}"
    big_kib="${ZMIN_STRESS_BIG_KIB:-16384}"
    ;;
  *)
    echo "unknown ZMIN_STRESS_PROFILE: $profile" >&2
    exit 2
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
zmin_bin="${ZMIN_BIN:-}"
if [[ -z "$zmin_bin" ]]; then
  rustup run stable cargo build --manifest-path "$repo_root/Cargo.toml" --release -p zmin-cli --bin zmin >/dev/null
  zmin_bin="$repo_root/target/release/zmin"
elif [[ "$zmin_bin" != /* ]]; then
  zmin_bin="$(cd "$repo_root" && pwd)/$zmin_bin"
fi

if [[ "${RUNNER_OS:-}" == "Windows" || "${OS:-}" == "Windows_NT" ]]; then
  if [[ ! -x "$zmin_bin" && -x "${zmin_bin}.exe" ]]; then
    zmin_bin="${zmin_bin}.exe"
  fi
else
  if [[ ! -x "$zmin_bin" && -x "${zmin_bin}.exe" ]]; then
    zmin_bin="${zmin_bin}.exe"
  fi
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

git_repo="$tmp_dir/git-repo"
zmin_repo="$tmp_dir/zmin-repo"

commit_env=(
  GIT_AUTHOR_NAME=Stress
  GIT_AUTHOR_EMAIL=stress@example.test
  GIT_COMMITTER_NAME=Stress
  GIT_COMMITTER_EMAIL=stress@example.test
  GIT_AUTHOR_DATE='1700000000 +0000'
  GIT_COMMITTER_DATE='1700000000 +0000'
)

git init --quiet -b main "$git_repo"
(cd "$zmin_repo" 2>/dev/null && true) || mkdir -p "$zmin_repo"
(cd "$zmin_repo" && "$zmin_bin" init --initial-branch=main >/dev/null)
for repo in "$git_repo" "$zmin_repo"; do
  git -C "$repo" config user.name Stress
  git -C "$repo" config user.email stress@example.test
  git -C "$repo" config commit.gpgsign false
done

write_fixture() {
  local repo="$1"
  mkdir -p "$repo/src" "$repo/docs" "$repo/deep/a/b/c/d/e" "$repo/bin"
  for idx in $(seq -w 1 "$files"); do
    numeric_idx=$((10#$idx))
    group="$(printf '%03d' $((numeric_idx % 32)))"
    mkdir -p "$repo/src/group-$group"
    {
      printf 'file %s\n' "$idx"
      printf 'group %s\n' "$group"
      printf 'payload %s %s\n' "$idx" "$profile"
    } >"$repo/src/group-$group/file-$idx.txt"
  done
  printf 'first line\nsecond line without newline' >"$repo/docs/no-newline.txt"
  printf '#!/usr/bin/env sh\nprintf stress\\n\n' >"$repo/bin/run.sh"
  chmod +x "$repo/bin/run.sh"
  dd if=/dev/zero of="$repo/bin/blob.bin" bs=1024 count="$big_kib" status=none
  printf 'deep path\n' >"$repo/deep/a/b/c/d/e/file.txt"
  if ln -s ../docs/no-newline.txt "$repo/src/link-to-no-newline" 2>/dev/null; then
    :
  fi
}

compare_output() {
  local label="$1"
  shift
  local git_out="$tmp_dir/git-$label.out"
  local zmin_out="$tmp_dir/zmin-$label.out"
  local git_norm="$tmp_dir/git-$label.norm"
  local zmin_norm="$tmp_dir/zmin-$label.norm"
  git -C "$git_repo" -c core.abbrev=7 "$@" >"$git_out"
  (cd "$zmin_repo" && "$zmin_bin" -c core.abbrev=7 "$@") >"$zmin_out"
  sed -E \
    -e 's/index ([0-9a-f]{7})[0-9a-f]*\.{2}([0-9a-f]{7})[0-9a-f]*/index \1..\2/g' \
    -e 's/^([0-9a-f]{7})[0-9a-f]* /\1 /' \
    "$git_out" >"$git_norm"
  sed -E \
    -e 's/index ([0-9a-f]{7})[0-9a-f]*\.{2}([0-9a-f]{7})[0-9a-f]*/index \1..\2/g' \
    -e 's/^([0-9a-f]{7})[0-9a-f]* /\1 /' \
    "$zmin_out" >"$zmin_norm"
  diff -u "$git_norm" "$zmin_norm"
  echo "ok: $label"
}

compare_status() {
  local label="$1"
  shift
  local git_status zmin_status
  set +e
  git -C "$git_repo" "$@" >/dev/null 2>&1
  git_status="$?"
  (cd "$zmin_repo" && "$zmin_bin" "$@" >/dev/null 2>&1)
  zmin_status="$?"
  set -e
  if [[ "$git_status" != "$zmin_status" ]]; then
    echo "status mismatch for $label: git=$git_status zmin=$zmin_status" >&2
    exit 1
  fi
  echo "ok: $label"
}

compare_git_state() {
  local label="$1"
  git -C "$git_repo" cat-file -p HEAD^{tree} >"$tmp_dir/git-tree.out"
  git -C "$zmin_repo" cat-file -p HEAD^{tree} >"$tmp_dir/zmin-tree.out"
  diff -u "$tmp_dir/git-tree.out" "$tmp_dir/zmin-tree.out"
  git -C "$git_repo" ls-files --stage >"$tmp_dir/git-index.out"
  git -C "$zmin_repo" ls-files --stage >"$tmp_dir/zmin-index.out"
  diff -u "$tmp_dir/git-index.out" "$tmp_dir/zmin-index.out"
  echo "ok: $label"
}

fixture_file() {
  local repo="$1"
  local group="$2"
  local path

  for path in "$repo/src/group-$group"/file-*; do
    if [[ -e "$path" ]]; then
      printf '%s' "${path#"$repo"/}"
      return 0
    fi
  done

  if [[ ! -e "$repo/src/group-$group" ]]; then
    echo "missing fixture file in $repo/src/group-$group" >&2
    exit 1
  fi
  echo "missing fixture file in $repo/src/group-$group" >&2
  exit 1
}

write_fixture "$git_repo"
write_fixture "$zmin_repo"

git -C "$git_repo" add -A
(cd "$zmin_repo" && "$zmin_bin" add -A)
git -C "$git_repo" ls-files --stage >"$tmp_dir/git-index-initial.out"
git -C "$zmin_repo" ls-files --stage >"$tmp_dir/zmin-index-initial.out"
diff -u "$tmp_dir/git-index-initial.out" "$tmp_dir/zmin-index-initial.out"
env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "initial stress"
(cd "$zmin_repo" && env "${commit_env[@]}" "$zmin_bin" commit -m "initial stress" >/dev/null)
compare_git_state "initial commit tree and index"

git_file_001="$(fixture_file "$git_repo" "001")"
zmin_file_001="$(fixture_file "$zmin_repo" "001")"
printf 'changed\n' >>"$git_repo/$git_file_001"
printf 'changed\n' >>"$zmin_repo/$zmin_file_001"

rm_file_002="$(fixture_file "$git_repo" "002")"
rm_file_002_zmin="$(fixture_file "$zmin_repo" "002")"
rm "$git_repo/$rm_file_002" "$zmin_repo/$rm_file_002_zmin"
mkdir -p "$git_repo/src/new" "$zmin_repo/src/new"
printf 'new tracked\n' >"$git_repo/src/new/new.txt"
printf 'new tracked\n' >"$zmin_repo/src/new/new.txt"
printf 'ignored by diff\n' >"$git_repo/untracked.tmp"
printf 'ignored by diff\n' >"$zmin_repo/untracked.tmp"

compare_output "status-porcelain" status --porcelain=v1 --branch
compare_output "diff-name-status" diff --name-status
compare_output "diff-stat" diff --stat
compare_output "diff-patch" diff "$git_file_001"
compare_status "diff-quiet" diff --quiet

git -C "$git_repo" add -A
(cd "$zmin_repo" && "$zmin_bin" add -A)
compare_output "diff-cached-name-status" diff --cached --name-status
compare_output "diff-cached-stat" diff --cached --stat
compare_output "grep-cached" grep --cached changed "$git_file_001"
env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "second stress"
(cd "$zmin_repo" && env "${commit_env[@]}" "$zmin_bin" commit -m "second stress" >/dev/null)
compare_git_state "second commit tree and index"

git -C "$git_repo" mv src/new/new.txt src/new/renamed.txt
(cd "$zmin_repo" && "$zmin_bin" mv src/new/new.txt src/new/renamed.txt)
rm_file_003="$(fixture_file "$git_repo" "003")"
rm_file_003_zmin="$(fixture_file "$zmin_repo" "003")"
git -C "$git_repo" rm -q "$rm_file_003"
(cd "$zmin_repo" && "$zmin_bin" rm "$rm_file_003_zmin" >/dev/null)
git -C "$git_repo" status --porcelain=v1 --no-renames >"$tmp_dir/git-mv-rm-status.out"
(cd "$zmin_repo" && "$zmin_bin" status --porcelain=v1) >"$tmp_dir/zmin-mv-rm-status.out"
diff -u "$tmp_dir/git-mv-rm-status.out" "$tmp_dir/zmin-mv-rm-status.out"
echo "ok: mv-rm-status"
git -C "$git_repo" add -A
(cd "$zmin_repo" && "$zmin_bin" add -A)
env "${commit_env[@]}" git -C "$git_repo" -c commit.gpgsign=false commit -qm "rename remove stress"
(cd "$zmin_repo" && env "${commit_env[@]}" "$zmin_bin" commit -m "rename remove stress" >/dev/null)
compare_git_state "rename remove commit tree and index"

compare_output "log-oneline" log --oneline --max-count 3
compare_output "rev-list-objects-count" rev-list --objects --count HEAD
compare_output "ls-tree-recursive" ls-tree -r --name-only HEAD

printf 'throwaway\n' >"$git_repo/throwaway.txt"
printf 'throwaway\n' >"$zmin_repo/throwaway.txt"
compare_output "clean-dry-run" clean -n
git -C "$git_repo" clean -f >/dev/null
(cd "$zmin_repo" && "$zmin_bin" clean -f >/dev/null)
compare_output "status-clean" status --porcelain=v1 --branch

git_pull_source="$tmp_dir/git-pull-source"
git_pull_client="$tmp_dir/git-pull-client"
zmin_pull_client="$tmp_dir/zmin-pull-client"
git init --quiet -b main "$git_pull_source"
git -C "$git_pull_source" config user.name Stress
git -C "$git_pull_source" config user.email stress@example.test
git -C "$git_pull_source" config commit.gpgsign false
printf 'base\n' >"$git_pull_source/base.txt"
git -C "$git_pull_source" add -A
env "${commit_env[@]}" git -C "$git_pull_source" -c commit.gpgsign=false commit -qm "base"
git -C "$tmp_dir" clone --quiet "$git_pull_source" "$git_pull_client"
(cd "$tmp_dir" && "$zmin_bin" clone "$git_pull_source" "$zmin_pull_client" >/dev/null)
for repo in "$git_pull_client" "$zmin_pull_client"; do
  git -C "$repo" config user.name Stress
  git -C "$repo" config user.email stress@example.test
  git -C "$repo" config commit.gpgsign false
  printf 'local\n' >"$repo/local.txt"
  git -C "$repo" add -A
  env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "local"
done
printf 'remote\n' >"$git_pull_source/remote.txt"
git -C "$git_pull_source" add -A
env "${commit_env[@]}" git -C "$git_pull_source" -c commit.gpgsign=false commit -qm "remote"
env "${commit_env[@]}" git -C "$git_pull_client" pull --rebase >/dev/null
(cd "$zmin_pull_client" && env "${commit_env[@]}" "$zmin_bin" pull --rebase >/dev/null)
git -C "$git_pull_client" cat-file -p HEAD^{tree} >"$tmp_dir/git-pull-rebase-tree.out"
git -C "$zmin_pull_client" cat-file -p HEAD^{tree} >"$tmp_dir/zmin-pull-rebase-tree.out"
diff -u "$tmp_dir/git-pull-rebase-tree.out" "$tmp_dir/zmin-pull-rebase-tree.out"
git -C "$git_pull_client" log --format=%s --max-count=3 >"$tmp_dir/git-pull-rebase-log.out"
git -C "$zmin_pull_client" log --format=%s --max-count=3 >"$tmp_dir/zmin-pull-rebase-log.out"
diff -u "$tmp_dir/git-pull-rebase-log.out" "$tmp_dir/zmin-pull-rebase-log.out"
git -C "$git_pull_client" status --porcelain=v1 --branch >"$tmp_dir/git-pull-rebase-status.out"
(cd "$zmin_pull_client" && "$zmin_bin" status --porcelain=v1 --branch) >"$tmp_dir/zmin-pull-rebase-status.out"
diff -u "$tmp_dir/git-pull-rebase-status.out" "$tmp_dir/zmin-pull-rebase-status.out"
echo "ok: pull-rebase-local"

compare_merge_case_state() {
  local label="$1"
  git -C "$git_case" status --porcelain=v1 >"$tmp_dir/git-$label-status.out"
  (cd "$zmin_case" && "$zmin_bin" status --porcelain=v1) >"$tmp_dir/zmin-$label-status.out"
  diff -u "$tmp_dir/git-$label-status.out" "$tmp_dir/zmin-$label-status.out"
  git -C "$git_case" write-tree >"$tmp_dir/git-$label-tree.out"
  git -C "$zmin_case" write-tree >"$tmp_dir/zmin-$label-tree.out"
  diff -u "$tmp_dir/git-$label-tree.out" "$tmp_dir/zmin-$label-tree.out"
  echo "ok: merge state $label"
}

compare_conflict_state() {
  local label="$1"
  local path="$2"
  git -C "$git_case" ls-files -u >"$tmp_dir/git-$label-unmerged.out"
  git -C "$zmin_case" ls-files -u >"$tmp_dir/zmin-$label-unmerged.out"
  diff -u "$tmp_dir/git-$label-unmerged.out" "$tmp_dir/zmin-$label-unmerged.out"
  git -C "$git_case" status --porcelain=v1 >"$tmp_dir/git-$label-status.out"
  (cd "$zmin_case" && "$zmin_bin" status --porcelain=v1) >"$tmp_dir/zmin-$label-status.out"
  diff -u "$tmp_dir/git-$label-status.out" "$tmp_dir/zmin-$label-status.out"
  if [[ -n "$path" ]]; then
    cmp "$git_case/$path" "$zmin_case/$path"
  fi
  echo "ok: merge conflict $label"
}

init_merge_case_pair() {
  local label="$1"
  git_case="$tmp_dir/git-merge-$label"
  zmin_case="$tmp_dir/zmin-merge-$label"
  git init --quiet -b main "$git_case"
  git init --quiet -b main "$zmin_case"
  for repo in "$git_case" "$zmin_case"; do
    git -C "$repo" config user.name Stress
    git -C "$repo" config user.email stress@example.test
    git -C "$repo" config commit.gpgsign false
  done
}

init_clean_merge_case_pair() {
  local label="$1"
  git_case="$tmp_dir/git-merge-$label"
  zmin_case="$tmp_dir/zmin-merge-$label"
  git init --quiet -b main "$git_case"
  git init --quiet -b main "$zmin_case"
  for repo in "$git_case" "$zmin_case"; do
    git -C "$repo" config user.name Stress
    git -C "$repo" config user.email stress@example.test
    git -C "$repo" config commit.gpgsign false
    printf 'base\n' >"$repo/a.txt"
    git -C "$repo" add a.txt
    env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "base file"
    git -C "$repo" switch -q -c feature
    printf 'feature\n' >"$repo/feature.txt"
    git -C "$repo" add feature.txt
    env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "feature file"
    git -C "$repo" switch -q main
    printf 'main\n' >"$repo/main.txt"
    git -C "$repo" add main.txt
    env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "main file"
  done
}

run_conflicting_merge_pair() {
  local label="$1"
  local git_status zmin_status
  set +e
  git -C "$git_case" merge feature >/dev/null 2>&1
  git_status="$?"
  (cd "$zmin_case" && "$zmin_bin" merge feature >/dev/null 2>&1)
  zmin_status="$?"
  set -e
  if [[ "$git_status" != "$zmin_status" ]]; then
    echo "merge status mismatch for $label: git=$git_status zmin=$zmin_status" >&2
    exit 1
  fi
}

init_clean_merge_case_pair "no-ff"
git -C "$git_case" merge --no-ff feature >/dev/null
(cd "$zmin_case" && "$zmin_bin" merge --no-ff feature >/dev/null)
git -C "$git_case" rev-list --parents -1 HEAD | cut -d' ' -f2- >"$tmp_dir/git-no-ff-parents.out"
git -C "$zmin_case" rev-list --parents -1 HEAD | cut -d' ' -f2- >"$tmp_dir/zmin-no-ff-parents.out"
diff -u "$tmp_dir/git-no-ff-parents.out" "$tmp_dir/zmin-no-ff-parents.out"
compare_merge_case_state "no-ff"

init_clean_merge_case_pair "no-commit"
git -C "$git_case" merge --no-commit feature >/dev/null 2>/dev/null
(cd "$zmin_case" && "$zmin_bin" merge --no-commit feature >/dev/null 2>/dev/null)
diff -u "$git_case/.git/MERGE_HEAD" "$zmin_case/.git/MERGE_HEAD"
compare_merge_case_state "no-commit"

init_clean_merge_case_pair "squash"
git -C "$git_case" merge --squash feature >/dev/null 2>/dev/null
(cd "$zmin_case" && "$zmin_bin" merge --squash feature >/dev/null 2>/dev/null)
if [[ -e "$zmin_case/.git/MERGE_HEAD" ]]; then
  echo "squash unexpectedly wrote MERGE_HEAD" >&2
  exit 1
fi
test -e "$zmin_case/.git/SQUASH_MSG"
compare_merge_case_state "squash"

init_merge_case_pair "binary"
for repo in "$git_case" "$zmin_case"; do
  printf 'base\0x' >"$repo/bin.dat"
  git -C "$repo" add bin.dat
  env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "base binary"
  git -C "$repo" switch -q -c feature
  printf 'feature\0x' >"$repo/bin.dat"
  env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -am "feature binary" -q
  git -C "$repo" switch -q main
  printf 'main\0x' >"$repo/bin.dat"
  env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -am "main binary" -q
done
run_conflicting_merge_pair "binary"
compare_conflict_state "binary" "bin.dat"

for delete_on_feature in yes no; do
  init_merge_case_pair "modify-delete-$delete_on_feature"
  for repo in "$git_case" "$zmin_case"; do
    printf 'base\n' >"$repo/a.txt"
    git -C "$repo" add a.txt
    env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "base file"
    git -C "$repo" switch -q -c feature
    if [[ "$delete_on_feature" == "yes" ]]; then
      git -C "$repo" rm -q a.txt
      env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "delete feature"
      git -C "$repo" switch -q main
      printf 'main\n' >"$repo/a.txt"
      git -C "$repo" add a.txt
      env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "modify main"
    else
      printf 'feature\n' >"$repo/a.txt"
      git -C "$repo" add a.txt
      env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "modify feature"
      git -C "$repo" switch -q main
      git -C "$repo" rm -q a.txt
      env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "delete main"
    fi
  done
  run_conflicting_merge_pair "modify-delete-$delete_on_feature"
  compare_conflict_state "modify-delete-$delete_on_feature" "a.txt"
done

for rename_on_feature in yes no; do
  init_merge_case_pair "rename-delete-$rename_on_feature"
  for repo in "$git_case" "$zmin_case"; do
    printf 'base\n' >"$repo/a.txt"
    git -C "$repo" add a.txt
    env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "base file"
    git -C "$repo" switch -q -c feature
    if [[ "$rename_on_feature" == "yes" ]]; then
      git -C "$repo" mv a.txt b.txt
      env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "rename feature"
      git -C "$repo" switch -q main
      git -C "$repo" rm -q a.txt
      env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "delete main"
    else
      git -C "$repo" rm -q a.txt
      env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "delete feature"
      git -C "$repo" switch -q main
      git -C "$repo" mv a.txt b.txt
      env "${commit_env[@]}" git -C "$repo" -c commit.gpgsign=false commit -qm "rename main"
    fi
  done
  run_conflicting_merge_pair "rename-delete-$rename_on_feature"
  compare_conflict_state "rename-delete-$rename_on_feature" "b.txt"
done

echo "git compatibility stress passed: profile=$profile files=$files big_kib=$big_kib"
