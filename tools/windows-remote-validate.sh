#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

ssh_target="${WINDOWS_SSH_TARGET:-}"
if [[ -z "$ssh_target" && $# -gt 0 && "$1" == *@* ]]; then
  ssh_target="$1"
  shift
fi

if [[ -z "$ssh_target" ]]; then
  cat >&2 <<'USAGE'
Usage:
  WINDOWS_SSH_TARGET=user@windows-host tools/windows-remote-validate.sh [mode] [native-args...]
  WINDOWS_SSH_TARGET=user@127.0.0.1 WINDOWS_SSH_PORT=2222 tools/windows-remote-validate.sh [mode] [native-args...]
  tools/windows-remote-validate.sh user@windows-host [mode] [native-args...]

Modes are passed to tools/windows-native-validate.ps1:
  targeted      fmt + one CLI integration case
  quick         fmt + CLI check + targeted test + release binary smoke
  integration   full CLI integration surface
  full          local equivalent of .github/workflows/windows-validation.yml

Examples:
  WINDOWS_SSH_TARGET=dev@192.168.64.10 tools/windows-remote-validate.sh targeted
  WINDOWS_SSH_TARGET=skron@127.0.0.1 WINDOWS_SSH_PORT=2222 tools/windows-remote-validate.sh targeted
  tools/windows-remote-validate.sh dev@192.168.64.10 targeted -TestFile git_cli_failure_compat -Case invalid_option_combinations_match_stock_git_failures
USAGE
  exit 2
fi

if [[ $# -gt 0 && "$1" != -* ]]; then
  mode="$1"
  shift
else
  mode="${WINDOWS_VALIDATE_MODE:-targeted}"
fi

native_args=("-Mode" "$mode" "$@")
job_id="skron-$(date -u +%Y%m%dT%H%M%SZ)-$$"
remote_archive_name=".${job_id}.tar.gz"
remote_script_name=".${job_id}.ps1"
remote_powershell="${WINDOWS_REMOTE_POWERSHELL:-powershell}"

ssh_opts=()
if [[ -n "${WINDOWS_SSH_OPTS:-}" ]]; then
  # shellcheck disable=SC2206
  ssh_opts=(${WINDOWS_SSH_OPTS})
fi
scp_opts=()
if [[ -n "${WINDOWS_SCP_OPTS:-}" ]]; then
  # shellcheck disable=SC2206
  scp_opts=(${WINDOWS_SCP_OPTS})
fi
if [[ -n "${WINDOWS_SSH_PORT:-}" ]]; then
  ssh_opts+=("-p" "$WINDOWS_SSH_PORT")
  scp_opts+=("-P" "$WINDOWS_SSH_PORT")
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

file_list="$tmp_dir/files.list"
archive="$tmp_dir/worktree.tar.gz"
remote_script="$tmp_dir/run-windows-validation.ps1"

git ls-files -z --cached --others --exclude-standard |
  while IFS= read -r -d '' path; do
    if [[ -e "$path" || -L "$path" ]]; then
      printf '%s\0' "$path"
    fi
  done >"$file_list"

if [[ ! -s "$file_list" ]]; then
  echo "No files found to package." >&2
  exit 1
fi

tar --null -T "$file_list" -czf "$archive"

ps_quote() {
  local value="${1//\'/\'\'}"
  printf "'%s'" "$value"
}

{
  echo '$ErrorActionPreference = "Stop"'
  echo 'Set-StrictMode -Version Latest'
  echo "\$JobId = $(ps_quote "$job_id")"
  echo "\$ArchiveName = $(ps_quote "$remote_archive_name")"
  echo "\$ScriptName = $(ps_quote "$remote_script_name")"
  echo '$HomeDir = if ($env:USERPROFILE) { $env:USERPROFILE } else { (Get-Location).Path }'
  echo '$RemoteRoot = if ($env:SKRON_WINDOWS_REMOTE_ROOT) { $env:SKRON_WINDOWS_REMOTE_ROOT } else { Join-Path $env:TEMP "skron-remote-validate" }'
  echo '$ArchivePath = Join-Path $HomeDir $ArchiveName'
  echo '$ScriptPath = Join-Path $HomeDir $ScriptName'
  echo '$JobRoot = Join-Path $RemoteRoot $JobId'
  echo '$TargetRoot = Join-Path $RemoteRoot "target"'
  printf '$NativeArgs = @('
  first=1
  for arg in "${native_args[@]}"; do
    if [[ $first -eq 0 ]]; then
      printf ', '
    fi
    ps_quote "$arg"
    first=0
  done
  echo ')'
  cat <<'PWSH'
$ExitCode = 0
try {
  if (Test-Path -LiteralPath $JobRoot) {
    Remove-Item -LiteralPath $JobRoot -Recurse -Force
  }
  New-Item -ItemType Directory -Force -Path $JobRoot | Out-Null
  New-Item -ItemType Directory -Force -Path $TargetRoot | Out-Null
  tar -xzf $ArchivePath -C $JobRoot
  if ($LASTEXITCODE -ne 0) {
    $ExitCode = $LASTEXITCODE
    throw "tar extraction failed with exit code $ExitCode"
  }
  Set-Location $JobRoot
  $env:CARGO_TARGET_DIR = $TargetRoot
  & ".\tools\windows-native-validate.ps1" @NativeArgs
  if ($LASTEXITCODE -ne 0) {
    $ExitCode = $LASTEXITCODE
    throw "windows-native-validate.ps1 failed with exit code $ExitCode"
  }
} catch {
  if ($ExitCode -eq 0) {
    $ExitCode = 1
  }
  Write-Error $_
} finally {
  if ($env:SKRON_WINDOWS_REMOTE_KEEP -ne "1") {
    if (Test-Path -LiteralPath $JobRoot) {
      Remove-Item -LiteralPath $JobRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $ArchivePath) {
      Remove-Item -LiteralPath $ArchivePath -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $ScriptPath) {
      Remove-Item -LiteralPath $ScriptPath -Force -ErrorAction SilentlyContinue
    }
  }
}
exit $ExitCode
PWSH
} >"$remote_script"

echo "Packaging current worktree for $ssh_target ($mode)..."
scp "${scp_opts[@]}" "$archive" "$ssh_target:$remote_archive_name"
scp "${scp_opts[@]}" "$remote_script" "$ssh_target:$remote_script_name"

echo "Running Windows native validation on $ssh_target..."
ssh "${ssh_opts[@]}" "$ssh_target" "$remote_powershell -NoProfile -ExecutionPolicy Bypass -File .\\$remote_script_name"
