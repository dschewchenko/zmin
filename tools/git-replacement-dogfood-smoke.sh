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
mkdir -p "$source_repo/dir"
printf 'nested\n' >"$source_repo/dir/nested.txt"
"$stock_git" -C "$source_repo" add -A
"$stock_git" -C "$source_repo" commit -m initial --quiet
"$stock_git" clone --bare "$source_repo" "$remote_repo" --quiet
"$stock_git" clone "$remote_repo" "$stock_client" --quiet
"$stock_git" clone "$remote_repo" "$zmin_client" --quiet

printf 'changed\n' >"$stock_client/tracked.txt"
printf 'changed\n' >"$zmin_client/tracked.txt"
printf 'new\n' >"$stock_client/new.txt"
printf 'new\n' >"$zmin_client/new.txt"

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
  for suffix in status stdout stderr; do
    if ! cmp -s "$stock_prefix.$suffix" "$zmin_prefix.$suffix"; then
      echo "mismatch for $label ($*): $suffix" >&2
      echo "--- stock $suffix" >&2
      od -An -tx1c "$stock_prefix.$suffix" >&2
      echo "--- zmin $suffix" >&2
      od -An -tx1c "$zmin_prefix.$suffix" >&2
      exit 1
    fi
  done
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
compare_command status_z status -z
compare_command status_v2_z_branch status --porcelain=v2 -z --branch
compare_command ls_files_z ls-files -z --cached --others --exclude-standard
compare_command ls_files_stage_z ls-files --stage -z
compare_command diff_name_status_z diff --name-status -z
compare_command rev_parse_git_dir rev-parse --git-dir
compare_command rev_parse_inside rev-parse --is-inside-work-tree
compare_command rev_parse_branch rev-parse --abbrev-ref HEAD
compare_command rev_parse_head rev-parse HEAD
compare_readonly_same_repo \
  rev_parse_nested_paths \
  "$zmin_client/dir" \
  rev-parse --show-prefix --show-cdup --show-toplevel
compare_command config_null_list config --null --list
compare_command config_remote_url config --get remote.origin.url
compare_command config_branch_remote config --get branch.main.remote
compare_command log_z log -z --format=%H%x00%P%x00%D%x00%s -1
compare_command log_date_iso_strict_z log -z --date=iso-strict --format=%H%x00%ad%x00%cd -1

"$stock_git" -C "$stock_client" add tracked.txt new.txt
"$stock_git" -C "$zmin_client" add tracked.txt new.txt
compare_command diff_cached_name_status_z diff --cached --name-status -z

compare_root_path_command rev_parse_toplevel rev-parse --show-toplevel

printf 'two\n' >"$source_repo/tracked.txt"
"$stock_git" -C "$source_repo" commit -am second --quiet
"$stock_git" -C "$source_repo" push "$remote_repo" main --quiet

run_capture stock "$stock_client" "$capture_dir/fetch.stock" fetch --prune --no-tags
run_capture zmin "$zmin_client" "$capture_dir/fetch.zmin" fetch --prune --no-tags
if [[ "$(cat "$capture_dir/fetch.stock.status")" != "$(cat "$capture_dir/fetch.zmin.status")" ]]; then
  echo "fetch exit mismatch" >&2
  exit 1
fi

compare_command fetched_origin_main rev-parse refs/remotes/origin/main

printf 'git_replacement_dogfood_smoke=ok\n'
