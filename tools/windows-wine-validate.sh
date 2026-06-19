#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

target="${WINDOWS_TARGET:-x86_64-pc-windows-gnu}"
validation_root="${WINDOWS_WINE_VALIDATION_ROOT:-$HOME/Downloads/zmin-windows-validation}"
portable_git="${GIT_FOR_WINDOWS_PORTABLE:-$validation_root/git-for-windows/portable}"
wine_prefix="${WINEPREFIX:-$validation_root/wine-prefix}"
wine_bin="${WINE64_BIN:-wine64}"

git_cmd_win="Z:${portable_git#/}/cmd"
git_bin_win="Z:${portable_git#/}/bin"
repo_root_win="Z:${repo_root#/}"
path_win="${git_cmd_win//\//\\};${git_bin_win//\//\\};C:\\windows\\system32;C:\\windows"
remote_http_win="${repo_root_win//\//\\}\\target\\$target\\release\\zmin-git-remote-http.exe"

if [[ ! -f "$portable_git/cmd/git.exe" ]]; then
  echo "Git for Windows portable is missing: $portable_git/cmd/git.exe" >&2
  exit 1
fi

if ! command -v "$wine_bin" >/dev/null 2>&1; then
  echo "wine64 is missing: $wine_bin" >&2
  exit 1
fi

export WINEDEBUG="${WINEDEBUG:--all}"
export WINEPREFIX="$wine_prefix"

cargo build -p zmin-cli --target "$target" --release --bins
cargo build -p zmin-git-remote-http --target "$target" --release
cargo test -p zmin-cli --target "$target" --release --test compatibility_command --no-run

zmin_exe="$repo_root/target/$target/release/zmin.exe"
git_wrapper_exe="$repo_root/target/$target/release/zmin.exe"
compatibility_exe="$(find "$repo_root/target/$target/release/deps" -maxdepth 1 -type f -name 'compatibility_command-*.exe' | sort | tail -n 1)"

env Path="$path_win" "$wine_bin" "$zmin_exe" --version
env Path="$path_win" "$wine_bin" "$git_wrapper_exe" --version
env Path="$path_win" ZMIN_GIT_REMOTE_HTTP="$remote_http_win" "$wine_bin" "$compatibility_exe" --nocapture

smoke_dir="$(mktemp -d "$validation_root/windows-wine-smoke.XXXXXX")"
trap 'rm -rf "$smoke_dir"' EXIT

run_zmin() {
  env Path="$path_win" "$wine_bin" "$zmin_exe" "$@"
}

run_zmin_commit() {
  env \
    Path="$path_win" \
    GIT_AUTHOR_NAME=Bench \
    GIT_AUTHOR_EMAIL=bench@example.test \
    GIT_AUTHOR_DATE='1700000000 +0000' \
    GIT_COMMITTER_NAME=Bench \
    GIT_COMMITTER_EMAIL=bench@example.test \
    GIT_COMMITTER_DATE='1700000000 +0000' \
    "$wine_bin" "$zmin_exe" "$@"
}

cd "$smoke_dir"
run_zmin init -b main
run_zmin config user.name Bench
run_zmin config user.email bench@example.test
run_zmin config commit.gpgsign false
printf 'hello from windows wine\n' > a.txt
run_zmin add -A
run_zmin_commit commit -m 'windows wine smoke'
run_zmin status --porcelain=v1 --branch > status.out
run_zmin log --oneline > log.out

if ! grep -qx '## main' status.out; then
  echo "unexpected Windows smoke status:" >&2
  cat status.out >&2
  exit 1
fi

if ! grep -q 'windows wine smoke' log.out; then
  echo "unexpected Windows smoke log:" >&2
  cat log.out >&2
  exit 1
fi

echo "ok: Windows Wine validation passed"
