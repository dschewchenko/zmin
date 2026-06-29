#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

zmin_bin="${ZMIN_BIN:-}"
if [[ -z "$zmin_bin" ]]; then
  if [[ -x "$repo_root/target/compat/zmin" ]]; then
    zmin_bin="$repo_root/target/compat/zmin"
  else
    cargo build -p zmin-cli --bin zmin --profile compat --quiet
    zmin_bin="$repo_root/target/compat/zmin"
  fi
fi

if [[ ! -x "$zmin_bin" ]]; then
  echo "ZMIN_BIN is not executable: $zmin_bin" >&2
  exit 1
fi

stock_git="${ZMIN_STOCK_GIT:-${GIT_BIN:-}}"
if [[ -z "$stock_git" ]]; then
  for candidate in /usr/bin/git /bin/git; do
    if [[ -x "$candidate" ]] && ! "$candidate" --version | grep -qi 'zmin'; then
      stock_git="$candidate"
      break
    fi
  done
fi

if [[ -z "$stock_git" ]]; then
  stock_git="$(command -v git || true)"
fi

if [[ -z "$stock_git" || ! -x "$stock_git" ]]; then
  echo "stock Git binary is not executable: ${stock_git:-<empty>}" >&2
  exit 1
fi

if "$stock_git" --version | grep -qi 'zmin'; then
  echo "stock Git binary resolved to Zmin shim: $stock_git" >&2
  echo "set ZMIN_STOCK_GIT to a stock Git binary" >&2
  exit 1
fi

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/zmin-git-replacement-smoke.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT

shim_dir="$tmp_dir/shim"
mkdir -p "$shim_dir"
cat >"$shim_dir/git" <<EOF
#!/usr/bin/env bash
exec "$zmin_bin" "\$@"
EOF
chmod +x "$shim_dir/git"

source_repo="$tmp_dir/source"
remote_repo="$tmp_dir/remote.git"
stock_client="$tmp_dir/stock-client"
zmin_client="$tmp_dir/zmin-client"
capture_dir="$tmp_dir/capture"
mkdir -p "$capture_dir"

"$stock_git" init -b main "$source_repo" --quiet
"$stock_git" -C "$source_repo" config user.name "Zmin Dogfood"
"$stock_git" -C "$source_repo" config user.email "zmin-dogfood@example.invalid"
"$stock_git" -C "$source_repo" config commit.gpgsign false
printf 'one\n' >"$source_repo/tracked.txt"
printf 'remove me\n' >"$source_repo/deleted.txt"
mkdir -p "$source_repo/dir"
printf 'nested\n' >"$source_repo/dir/nested.txt"
"$stock_git" -C "$source_repo" add -A
"$stock_git" -C "$source_repo" commit -m initial --quiet
"$stock_git" clone --bare "$source_repo" "$remote_repo" --quiet
"$stock_git" clone "$remote_repo" "$stock_client" --quiet
"$stock_git" clone "$remote_repo" "$zmin_client" --quiet
"$stock_git" -C "$stock_client" lfs install --local --skip-repo >/dev/null
PATH="$shim_dir:$PATH" git -C "$zmin_client" lfs install --local --skip-repo >/dev/null

printf 'changed\n' >"$stock_client/tracked.txt"
printf 'changed\n' >"$zmin_client/tracked.txt"
printf 'new\n' >"$stock_client/new.txt"
printf 'new\n' >"$zmin_client/new.txt"
rm "$stock_client/deleted.txt" "$zmin_client/deleted.txt"

run_capture() {
  local tool="$1"
  local cwd="$2"
  local prefix="$3"
  shift 3
  set +e
  if [[ "$tool" == "stock" ]]; then
    "$stock_git" -C "$cwd" "$@" >"$prefix.stdout" 2>"$prefix.stderr"
  else
    PATH="$shim_dir:$PATH" git -C "$cwd" "$@" >"$prefix.stdout" 2>"$prefix.stderr"
  fi
  local code=$?
  set -e
  printf '%s\n' "$code" >"$prefix.status"
}

compare_capture_prefixes() {
  local label="$1"
  local stock_prefix="$2"
  local zmin_prefix="$3"
  for suffix in status stdout stderr; do
    if ! cmp -s "$stock_prefix.$suffix" "$zmin_prefix.$suffix"; then
      echo "mismatch for $label: $suffix" >&2
      echo "--- stock $suffix" >&2
      od -An -tx1c "$stock_prefix.$suffix" >&2
      echo "--- zmin $suffix" >&2
      od -An -tx1c "$zmin_prefix.$suffix" >&2
      exit 1
    fi
  done
}

compare_nul_sorted_stdout_prefixes() {
  local label="$1"
  local stock_prefix="$2"
  local zmin_prefix="$3"
  for suffix in status stderr; do
    if ! cmp -s "$stock_prefix.$suffix" "$zmin_prefix.$suffix"; then
      echo "mismatch for $label: $suffix" >&2
      echo "--- stock $suffix" >&2
      od -An -tx1c "$stock_prefix.$suffix" >&2
      echo "--- zmin $suffix" >&2
      od -An -tx1c "$zmin_prefix.$suffix" >&2
      exit 1
    fi
  done
  python3 - "$stock_prefix.stdout" "$zmin_prefix.stdout" "$label" <<'PY'
import pathlib
import sys

stock = pathlib.Path(sys.argv[1]).read_bytes().split(b"\0")
zmin = pathlib.Path(sys.argv[2]).read_bytes().split(b"\0")
label = sys.argv[3]

if stock and stock[-1] == b"":
    stock.pop()
if zmin and zmin[-1] == b"":
    zmin.pop()

if sorted(stock) != sorted(zmin):
    print(f"mismatch for {label}: stdout", file=sys.stderr)
    print("--- stock stdout", file=sys.stderr)
    for item in sorted(stock):
        print(repr(item), file=sys.stderr)
    print("--- zmin stdout", file=sys.stderr)
    for item in sorted(zmin):
        print(repr(item), file=sys.stderr)
    sys.exit(1)
PY
}

compare_capture_prefixes_with_normalized_stderr() {
  local label="$1"
  local stock_prefix="$2"
  local zmin_prefix="$3"
  local stock_stderr="$stock_prefix.stderr.normalized"
  local zmin_stderr="$zmin_prefix.stderr.normalized"
  sed \
    -e 's/workflow-stock/workflow-remote/g' \
    -e 's/workflow-zmin/workflow-remote/g' \
    -e 's/workflow-stock\.git/workflow-remote.git/g' \
    -e 's/workflow-zmin\.git/workflow-remote.git/g' \
    "$stock_prefix.stderr" >"$stock_stderr"
  sed \
    -e 's/workflow-stock/workflow-remote/g' \
    -e 's/workflow-zmin/workflow-remote/g' \
    -e 's/workflow-stock\.git/workflow-remote.git/g' \
    -e 's/workflow-zmin\.git/workflow-remote.git/g' \
    "$zmin_prefix.stderr" >"$zmin_stderr"
  if ! cmp -s "$stock_prefix.status" "$zmin_prefix.status"; then
    echo "mismatch for $label: status" >&2
    echo "--- stock status" >&2
    od -An -tx1c "$stock_prefix.status" >&2
    echo "--- zmin status" >&2
    od -An -tx1c "$zmin_prefix.status" >&2
    exit 1
  fi
  if ! cmp -s "$stock_prefix.stdout" "$zmin_prefix.stdout"; then
    echo "mismatch for $label: stdout" >&2
    echo "--- stock stdout" >&2
    od -An -tx1c "$stock_prefix.stdout" >&2
    echo "--- zmin stdout" >&2
    od -An -tx1c "$zmin_prefix.stdout" >&2
    exit 1
  fi
  if ! cmp -s "$stock_stderr" "$zmin_stderr"; then
    echo "mismatch for $label: stderr" >&2
    echo "--- stock stderr" >&2
    od -An -tx1c "$stock_prefix.stderr" >&2
    echo "--- zmin stderr" >&2
    od -An -tx1c "$zmin_prefix.stderr" >&2
    exit 1
  fi
}

compare_command() {
  local label="$1"
  shift
  compare_command_at "$label" "$stock_client" "$zmin_client" "$@"
}

compare_command_at() {
  local label="$1"
  local stock_cwd="$2"
  local zmin_cwd="$3"
  shift 3
  local stock_prefix="$capture_dir/$label.stock"
  local zmin_prefix="$capture_dir/$label.zmin"
  run_capture stock "$stock_cwd" "$stock_prefix" "$@"
  run_capture zmin "$zmin_cwd" "$zmin_prefix" "$@"
  compare_capture_prefixes "$label ($*)" "$stock_prefix" "$zmin_prefix"
}

compare_readonly_same_repo() {
  local label="$1"
  local cwd="$2"
  shift 2
  compare_command_at "$label" "$cwd" "$cwd" "$@"
}

compare_root_path_command() {
  local label="$1"
  shift
  local stock_prefix="$capture_dir/$label.stock"
  local prefix="$capture_dir/$label.zmin"
  run_capture stock "$stock_client" "$stock_prefix" "$@"
  run_capture zmin "$zmin_client" "$prefix" "$@"
  if [[ "$(cat "$stock_prefix.status")" != "$(cat "$prefix.status")" ]]; then
    echo "status mismatch for $label: $*" >&2
    exit 1
  fi
  if [[ "$(cat "$prefix.status")" != "0" ]]; then
    echo "zmin command failed for $label: $*" >&2
    cat "$prefix.stderr" >&2
    exit 1
  fi
  if ! cmp -s "$stock_prefix.stderr" "$prefix.stderr"; then
    echo "stderr mismatch for $label: $*" >&2
    exit 1
  fi
  local stock_expected
  local zmin_expected
  stock_expected="$(cd "$stock_client" && pwd -P)"
  zmin_expected="$(cd "$zmin_client" && pwd -P)"
  if [[ "$(cat "$stock_prefix.stdout")" != "$stock_expected" ]]; then
    echo "unexpected stock stdout for $label" >&2
    printf 'expected: %s\nactual: %s\n' "$stock_expected" "$(cat "$stock_prefix.stdout")" >&2
    exit 1
  fi
  if [[ "$(cat "$prefix.stdout")" != "$zmin_expected" ]]; then
    echo "unexpected zmin stdout for $label" >&2
    printf 'expected: %s\nactual: %s\n' "$zmin_expected" "$(cat "$prefix.stdout")" >&2
    exit 1
  fi
}

version_output="$(PATH="$shim_dir:$PATH" git --version)"
case "$version_output" in
  'git version 2.47.1.zmin '*)
    ;;
  *)
    echo "unexpected git shim version: $version_output" >&2
    exit 1
    ;;
esac

short_version_output="$(PATH="$shim_dir:$PATH" git -v)"
if [[ "$short_version_output" != "$version_output" ]]; then
  echo "git -v did not match git --version" >&2
  printf 'git --version: %s\n' "$version_output" >&2
  printf 'git -v: %s\n' "$short_version_output" >&2
  exit 1
fi

lfs_version_output="$(PATH="$shim_dir:$PATH" git -C "$zmin_client" lfs version)"
case "$lfs_version_output" in
  'git-lfs/zmin (zmin '*'; built-in local foundation)')
    ;;
  *)
    echo "unexpected git lfs version through shim: $lfs_version_output" >&2
    exit 1
    ;;
esac

lfs_env_prefix="$capture_dir/lfs_env.zmin"
run_capture zmin "$zmin_client" "$lfs_env_prefix" lfs env
if [[ "$(cat "$lfs_env_prefix.status")" != "0" ]]; then
  echo "git lfs env failed through shim" >&2
  cat "$lfs_env_prefix.stderr" >&2
  exit 1
fi
zmin_client_real="$(cd "$zmin_client" && pwd -P)"
for expected in \
  "git-lfs/zmin (zmin " \
  "git version 2.47.1.zmin" \
  "LocalWorkingDir=$zmin_client_real" \
  "LocalGitDir=$zmin_client_real/.git" \
  "LocalMediaDir=$zmin_client_real/.git/lfs/objects" \
  "git config"; do
  if ! grep -Fq "$expected" "$lfs_env_prefix.stdout"; then
    echo "git lfs env missing '$expected' through shim" >&2
    cat "$lfs_env_prefix.stdout" >&2
    exit 1
  fi
done
if [[ -s "$lfs_env_prefix.stderr" ]]; then
  echo "git lfs env wrote stderr through shim" >&2
  cat "$lfs_env_prefix.stderr" >&2
  exit 1
fi

compare_readonly_same_repo lfs_ls_files_empty "$zmin_client" lfs ls-files
compare_readonly_same_repo lfs_ls_files_name_only_empty "$zmin_client" lfs ls-files --name-only

build_options_prefix="$capture_dir/version_build_options.zmin"
run_capture zmin "$zmin_client" "$build_options_prefix" version --build-options
if [[ "$(cat "$build_options_prefix.status")" != "0" ]]; then
  echo "git version --build-options failed through shim" >&2
  cat "$build_options_prefix.stderr" >&2
  exit 1
fi
for expected in \
  "git version 2.47.1.zmin " \
  "cpu:" \
  "sizeof-long:" \
  "sizeof-size_t:" \
  "shell-path:" \
  "zmin-version:"; do
  if ! grep -Fq "$expected" "$build_options_prefix.stdout"; then
    echo "git version --build-options missing '$expected'" >&2
    cat "$build_options_prefix.stdout" >&2
    exit 1
  fi
done
if [[ -s "$build_options_prefix.stderr" ]]; then
  echo "git version --build-options wrote stderr through shim" >&2
  cat "$build_options_prefix.stderr" >&2
  exit 1
fi

compare_command version_invalid version --version

compare_command status_short status --short
compare_command status_short_branch status --short --branch
compare_command status_z status -z
compare_command status_v2_z_branch status --porcelain=v2 -z --branch
compare_command status_v2_z_branch_untracked_no status --porcelain=v2 -z --branch --untracked-files=no
printf 'ignored.log\n' >"$stock_client/.gitignore"
printf 'ignored.log\n' >"$zmin_client/.gitignore"
printf 'ignored\n' >"$stock_client/ignored.log"
printf 'ignored\n' >"$zmin_client/ignored.log"
compare_command status_ignored_porcelain_z status --ignored --porcelain=v1 -z
compare_command status_ignored_v2_z_branch status --ignored --porcelain=v2 -z --branch
rm "$stock_client/.gitignore" "$zmin_client/.gitignore" \
  "$stock_client/ignored.log" "$zmin_client/ignored.log"
compare_command ls_files_z ls-files -z --cached --others --exclude-standard
compare_command ls_files_stage_z ls-files --stage -z
compare_command ls_files_cached_pathspec_z ls-files -z --cached -- dir
printf 'loose\n' >"$stock_client/dir/loose.txt"
printf 'loose\n' >"$zmin_client/dir/loose.txt"
compare_command ls_files_others_pathspec_z ls-files -z --others --exclude-standard -- dir
rm "$stock_client/dir/loose.txt" "$zmin_client/dir/loose.txt"
compare_command ls_files_deleted_modified_z ls-files -z --deleted --modified
compare_command ls_files_worktree_mix_z ls-files -z --modified --deleted --others --exclude-standard
compare_command diff_name_status_z diff --name-status -z
compare_command diff_name_only_z diff --name-only -z
compare_command diff_raw_z diff --raw -z
compare_command rev_parse_git_dir rev-parse --git-dir
compare_command rev_parse_inside rev-parse --is-inside-work-tree
compare_command rev_parse_branch rev-parse --abbrev-ref HEAD
compare_command rev_parse_head rev-parse HEAD
compare_readonly_same_repo \
  rev_parse_nested_paths \
  "$zmin_client/dir" \
  rev-parse --show-prefix --show-cdup --show-toplevel
compare_readonly_same_repo \
  rev_parse_nested_cdup \
  "$zmin_client/dir" \
  rev-parse --show-cdup
compare_readonly_same_repo \
  rev_parse_nested_prefix \
  "$zmin_client/dir" \
  rev-parse --show-prefix
compare_readonly_same_repo \
  rev_parse_nested_toplevel \
  "$zmin_client/dir" \
  rev-parse --show-toplevel
run_capture stock "$stock_client" "$capture_dir/config_null_list.stock" config --null --list
run_capture zmin "$zmin_client" "$capture_dir/config_null_list.zmin" config --null --list
compare_nul_sorted_stdout_prefixes \
  "config_null_list" \
  "$capture_dir/config_null_list.stock" \
  "$capture_dir/config_null_list.zmin"
compare_command config_core_filemode config --get core.filemode
compare_command config_remote_url config --get remote.origin.url
compare_command config_branch_remote config --get branch.main.remote
compare_command config_branch_merge config --get branch.main.merge
compare_command config_missing_commit_template config --get commit.template
compare_command config_remote_get_regexp config --get-regexp '^remote\.'
compare_command config_branch_get_regexp config --get-regexp '^branch\.'
compare_command log_z log -z --format=%H%x00%P%x00%D%x00%s -1
compare_command log_date_iso_strict_z log -z --date=iso-strict --format=%H%x00%ad%x00%cd -1
compare_command log_pathspec_dir_z log -z --format=%H%x00%s -1 -- dir

printf 'nested changed\n' >"$stock_client/dir/nested.txt"
printf 'nested changed\n' >"$zmin_client/dir/nested.txt"
compare_command diff_pathspec_dir_name_status_z diff --name-status -z -- dir
compare_command diff_pathspec_dir_name_only_z diff --name-only -z -- dir
compare_command status_short_pathspec_dir status --short -- dir
compare_command status_pathspec_dir_z status --porcelain=v1 -z -- dir

"$stock_git" -C "$stock_client" add tracked.txt new.txt
"$stock_git" -C "$zmin_client" add tracked.txt new.txt
compare_command diff_cached_pathspec_name_status_z diff --cached --name-status -z -- new.txt
compare_command diff_cached_name_status_z diff --cached --name-status -z
compare_command diff_cached_name_only_z diff --cached --name-only -z
compare_command diff_cached_pathspec_name_only_z diff --cached --name-only -z -- new.txt
compare_command diff_cached_raw_z diff --cached --raw -z
compare_command diff_cached_pathspec_raw_z diff --cached --raw -z -- new.txt
compare_command diff_cached_modified_pathspec_raw_z diff --cached --raw -z -- tracked.txt

compare_root_path_command rev_parse_toplevel rev-parse --show-toplevel

printf 'two\n' >"$source_repo/tracked.txt"
"$stock_git" -C "$source_repo" commit -am second --quiet
"$stock_git" -C "$source_repo" tag later-tag
"$stock_git" -C "$source_repo" push "$remote_repo" main --quiet
"$stock_git" -C "$source_repo" push "$remote_repo" later-tag --quiet
"$stock_git" -C "$stock_client" update-ref refs/remotes/origin/gone HEAD
"$stock_git" -C "$zmin_client" update-ref refs/remotes/origin/gone HEAD

run_capture stock "$stock_client" "$capture_dir/fetch_prune_no_tags.stock" fetch --prune --no-tags
run_capture zmin "$zmin_client" "$capture_dir/fetch_prune_no_tags.zmin" fetch --prune --no-tags
compare_capture_prefixes \
  "fetch_prune_no_tags" \
  "$capture_dir/fetch_prune_no_tags.stock" \
  "$capture_dir/fetch_prune_no_tags.zmin"

compare_command fetched_origin_main rev-parse refs/remotes/origin/main
compare_command fetched_pruned_branch_missing rev-parse --verify refs/remotes/origin/gone
compare_command fetched_no_tags_missing rev-parse --verify refs/tags/later-tag
if ! cmp -s "$stock_client/.git/FETCH_HEAD" "$zmin_client/.git/FETCH_HEAD"; then
  echo "FETCH_HEAD mismatch after fetch --prune --no-tags" >&2
  exit 1
fi

workflow_stock_remote="$tmp_dir/workflow-stock.git"
workflow_zmin_remote="$tmp_dir/workflow-zmin.git"
workflow_stock_publish="$tmp_dir/workflow-stock-publish"
workflow_zmin_publish="$tmp_dir/workflow-zmin-publish"
workflow_stock_pull="$tmp_dir/workflow-stock-pull"
workflow_zmin_pull="$tmp_dir/workflow-zmin-pull"

"$stock_git" clone --bare "$source_repo" "$workflow_stock_remote" --quiet
"$stock_git" clone --bare "$source_repo" "$workflow_zmin_remote" --quiet
"$stock_git" clone "$workflow_stock_remote" "$workflow_stock_publish" --quiet
"$stock_git" clone "$workflow_stock_remote" "$workflow_stock_pull" --quiet
PATH="$shim_dir:$PATH" git clone "$workflow_zmin_remote" "$workflow_zmin_publish" --quiet
PATH="$shim_dir:$PATH" git clone "$workflow_zmin_remote" "$workflow_zmin_pull" --quiet

for repo in \
  "$workflow_stock_publish" \
  "$workflow_stock_pull" \
  "$workflow_zmin_publish" \
  "$workflow_zmin_pull"; do
  "$stock_git" -C "$repo" config user.name "Zmin Dogfood"
  "$stock_git" -C "$repo" config user.email "zmin-dogfood@example.invalid"
  "$stock_git" -C "$repo" config commit.gpgsign false
done

printf 'workflow stock\n' >"$workflow_stock_publish/tracked.txt"
printf 'workflow stock\n' >"$workflow_zmin_publish/tracked.txt"
printf 'workflow-new\n' >"$workflow_stock_publish/workflow.txt"
printf 'workflow-new\n' >"$workflow_zmin_publish/workflow.txt"
"$stock_git" -C "$workflow_stock_publish" add tracked.txt workflow.txt
PATH="$shim_dir:$PATH" git -C "$workflow_zmin_publish" add tracked.txt workflow.txt

workflow_env=(
  GIT_AUTHOR_NAME="Zmin Dogfood"
  GIT_AUTHOR_EMAIL="zmin-dogfood@example.invalid"
  GIT_COMMITTER_NAME="Zmin Dogfood"
  GIT_COMMITTER_EMAIL="zmin-dogfood@example.invalid"
  GIT_AUTHOR_DATE="2000-01-02T03:04:05Z"
  GIT_COMMITTER_DATE="2000-01-02T03:04:05Z"
)

env "${workflow_env[@]}" \
  "$stock_git" -C "$workflow_stock_publish" commit -m "workflow update" \
  >"$capture_dir/workflow_commit.stock.stdout" \
  2>"$capture_dir/workflow_commit.stock.stderr"
printf '0\n' >"$capture_dir/workflow_commit.stock.status"
env "${workflow_env[@]}" PATH="$shim_dir:$PATH" \
  git -C "$workflow_zmin_publish" commit -m "workflow update" \
  >"$capture_dir/workflow_commit.zmin.stdout" \
  2>"$capture_dir/workflow_commit.zmin.stderr"
printf '0\n' >"$capture_dir/workflow_commit.zmin.status"
compare_capture_prefixes \
  "workflow_commit" \
  "$capture_dir/workflow_commit.stock" \
  "$capture_dir/workflow_commit.zmin"

if [[ "$("$stock_git" -C "$workflow_stock_publish" rev-parse HEAD)" != \
      "$(PATH="$shim_dir:$PATH" git -C "$workflow_zmin_publish" rev-parse HEAD)" ]]; then
  echo "workflow commit HEAD mismatch between stock and shim publishers" >&2
  exit 1
fi

run_capture stock "$workflow_stock_publish" "$capture_dir/workflow_push.stock" push origin main
run_capture zmin "$workflow_zmin_publish" "$capture_dir/workflow_push.zmin" push origin main
compare_capture_prefixes_with_normalized_stderr \
  "workflow_push" \
  "$capture_dir/workflow_push.stock" \
  "$capture_dir/workflow_push.zmin"

run_capture stock "$workflow_stock_pull" "$capture_dir/workflow_pull.stock" pull --ff-only
run_capture zmin "$workflow_zmin_pull" "$capture_dir/workflow_pull.zmin" pull --ff-only
compare_capture_prefixes_with_normalized_stderr \
  "workflow_pull" \
  "$capture_dir/workflow_pull.stock" \
  "$capture_dir/workflow_pull.zmin"

if [[ "$(cat "$workflow_stock_pull/tracked.txt")" != "workflow stock" ]]; then
  echo "unexpected stock workflow pull content" >&2
  exit 1
fi
if [[ "$(cat "$workflow_zmin_pull/tracked.txt")" != "workflow stock" ]]; then
  echo "unexpected zmin workflow pull content" >&2
  exit 1
fi
if [[ "$(cat "$workflow_stock_pull/workflow.txt")" != "workflow-new" ]]; then
  echo "missing stock workflow-added file after pull" >&2
  exit 1
fi
if [[ "$(cat "$workflow_zmin_pull/workflow.txt")" != "workflow-new" ]]; then
  echo "missing zmin workflow-added file after pull" >&2
  exit 1
fi
if [[ "$("$stock_git" -C "$workflow_stock_pull" rev-parse HEAD)" != \
      "$(PATH="$shim_dir:$PATH" git -C "$workflow_zmin_pull" rev-parse HEAD)" ]]; then
  echo "workflow pull HEAD mismatch between stock and shim subscribers" >&2
  exit 1
fi

printf 'git_replacement_dogfood_smoke=ok\n'
