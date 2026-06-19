param(
  [switch]$InstallMissingTools
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Test-Admin {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = [Security.Principal.WindowsPrincipal]::new($identity)
  $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-Checked {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Label,

    [Parameter(Mandatory = $true)]
    [string]$FilePath,

    [string[]]$Arguments = @()
  )

  Write-Host "+ $Label"
  & $FilePath @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "$Label failed with exit code $LASTEXITCODE"
  }
}

function Install-WithWinget {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Id
  )

  if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    throw "winget is not available; install $Id manually or enable App Installer."
  }

  Invoke-Checked `
    -Label "winget install $Id" `
    -FilePath "winget" `
    -Arguments @(
      "install",
      "--id",
      $Id,
      "-e",
      "--source",
      "winget",
      "--accept-package-agreements",
      "--accept-source-agreements"
    )
}

if (-not (Test-Admin)) {
  throw "Run this script from an elevated PowerShell session."
}

$sshCapability = Get-WindowsCapability -Online -Name "OpenSSH.Server~~~~0.0.1.0"
if ($sshCapability.State -ne "Installed") {
  Add-WindowsCapability -Online -Name "OpenSSH.Server~~~~0.0.1.0" | Out-Null
}

Set-Service -Name sshd -StartupType Automatic
Start-Service -Name sshd

if (-not (Get-NetFirewallRule -Name "OpenSSH-Server-In-TCP" -ErrorAction SilentlyContinue)) {
  New-NetFirewallRule `
    -Name "OpenSSH-Server-In-TCP" `
    -DisplayName "OpenSSH Server (sshd)" `
    -Enabled True `
    -Direction Inbound `
    -Protocol TCP `
    -Action Allow `
    -LocalPort 22 | Out-Null
}

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
  if ($InstallMissingTools) {
    Install-WithWinget -Id "Git.Git"
  } else {
    throw "Git is missing. Re-run with -InstallMissingTools or install Git for Windows manually."
  }
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  if ($InstallMissingTools) {
    Install-WithWinget -Id "Rustlang.Rustup"
  } else {
    throw "Cargo/Rust is missing. Re-run with -InstallMissingTools or install Rustup manually."
  }
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
  if (Test-Path -LiteralPath $cargoBin) {
    $env:Path = "$cargoBin;$env:Path"
  }
}

Invoke-Checked -Label "rustup update stable" -FilePath "rustup" -Arguments @("update", "stable")
Invoke-Checked -Label "rustup default stable" -FilePath "rustup" -Arguments @("default", "stable")
Invoke-Checked -Label "rustup component add rustfmt clippy" -FilePath "rustup" -Arguments @("component", "add", "rustfmt", "clippy")

Write-Host ""
Write-Host "Tool versions:"
Invoke-Checked -Label "git --version" -FilePath "git" -Arguments @("--version")
Invoke-Checked -Label "rustc --version" -FilePath "rustc" -Arguments @("--version")
Invoke-Checked -Label "cargo --version" -FilePath "cargo" -Arguments @("--version")

$ipv4 = Get-NetIPAddress -AddressFamily IPv4 |
  Where-Object {
    $_.IPAddress -notlike "127.*" -and
    $_.IPAddress -notlike "169.254.*" -and
    $_.PrefixOrigin -ne "WellKnown"
  } |
  Select-Object -ExpandProperty IPAddress

Write-Host ""
Write-Host "OpenSSH Server is enabled."
Write-Host "Windows user: $env:USERNAME"
Write-Host "Candidate IPv4 addresses:"
$ipv4 | ForEach-Object { Write-Host "  $_" }
Write-Host ""
Write-Host "Use from macOS:"
Write-Host "  WINDOWS_SSH_TARGET=$env:USERNAME@<ip> tools/windows-remote-validate.sh targeted"
