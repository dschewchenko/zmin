param(
  [ValidateSet("quick", "standard", "exhaustive")]
  [string]$Mode = "quick",

  [switch]$SkipBuild,

  [switch]$SkipBenchmark,

  [int]$BenchmarkRepeats = 0,

  [string]$BuildProfile = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $RepoRoot

$env:GIT_CONFIG_NOSYSTEM = "1"
$env:GIT_TERMINAL_PROMPT = "0"

if (-not $env:CARGO_TARGET_DIR) {
  $env:CARGO_TARGET_DIR = Join-Path $RepoRoot "target"
}

if (-not $BuildProfile) {
  $BuildProfile = if ($env:ZMIN_WINDOWS_EXTENDED_BUILD_PROFILE) { $env:ZMIN_WINDOWS_EXTENDED_BUILD_PROFILE } else { "release" }
}

$BuildDir = Join-Path $env:CARGO_TARGET_DIR $BuildProfile
$ZminGitExe = Join-Path $BuildDir "zmin.exe"

function Get-CargoBuildArgs {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Package,

    [string[]]$ExtraArgs = @()
  )

  $args = @("build", "-p", $Package)
  if ($BuildProfile -eq "release") {
    $args += "--release"
  } else {
    $args += @("--profile", $BuildProfile)
  }
  $args += $ExtraArgs
  return $args
}

function Bash-Quote {
  param([string]$Value)
  return "'" + ($Value -replace "'", "'\''") + "'"
}

function Convert-ToBashPath {
  param([string]$Path)
  $quoted = Bash-Quote $Path
  $result = & bash -lc "cygpath -u $quoted"
  if ($LASTEXITCODE -ne 0) {
    throw "failed to convert path for Git Bash: $Path"
  }
  return ($result | Select-Object -First 1).Trim()
}

function Invoke-Checked {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Label,

    [Parameter(Mandatory = $true)]
    [string]$FilePath,

    [string[]]$Arguments = @()
  )

  Write-Host "::group::$Label"
  Write-Host "+ $FilePath $($Arguments -join ' ')"
  try {
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
      throw "$Label failed with exit code $LASTEXITCODE"
    }
  } finally {
    Write-Host "::endgroup::"
  }
}

function Invoke-BashChecked {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Label,

    [Parameter(Mandatory = $true)]
    [string]$Command,

    [hashtable]$Env = @{}
  )

  $assignments = @()
  foreach ($item in $Env.GetEnumerator()) {
    $assignments += "$($item.Key)=$(Bash-Quote ([string]$item.Value))"
  }

  $fullCommand = "cd $(Bash-Quote $RepoBash) && env $($assignments -join ' ') $Command"

  Write-Host "::group::$Label"
  Write-Host "+ bash -lc $fullCommand"
  try {
    & bash -lc $fullCommand
    if ($LASTEXITCODE -ne 0) {
      throw "$Label failed with exit code $LASTEXITCODE"
    }
  } finally {
    Write-Host "::endgroup::"
  }
}

function Run-RealRepoSmoke {
  param([string]$RepoUrl)

  Invoke-BashChecked `
    -Label "Real repo smoke: $RepoUrl" `
    -Command "./tools/git-real-repo-smoke.sh $(Bash-Quote $RepoUrl)" `
    -Env @{
      ZMIN_BIN = $ZminGitBash
      RUNNER_OS = "Windows"
      GIT_TERMINAL_PROMPT = "0"
    }
}

Write-Host "Windows native extended compatibility mode: $Mode"
Invoke-Checked -Label "Tool versions" -FilePath "cmd" -Arguments @("/c", "rustc --version && cargo --version && git --version && bash --version")

if (-not $SkipBuild) {
  Invoke-Checked -Label "Build Windows CLI binaries ($BuildProfile)" -FilePath "cargo" -Arguments (Get-CargoBuildArgs -Package "zmin-cli" -ExtraArgs @("--bins"))
  Invoke-Checked -Label "Build Windows remote helper ($BuildProfile)" -FilePath "cargo" -Arguments (Get-CargoBuildArgs -Package "zmin-git-remote-http")
}

if (-not (Test-Path -LiteralPath $ZminGitExe)) {
  throw "missing zmin release binary: $ZminGitExe"
}

$RepoBash = Convert-ToBashPath $RepoRoot
$ZminGitBash = Convert-ToBashPath $ZminGitExe

switch ($Mode) {
  "quick" {
    $stressProfile = "small"
    $providerOnly = "github,gitlab"
    $providerRemoteOnly = "1"
    $realRepos = @("https://github.com/octocat/Hello-World.git")
    if ($BenchmarkRepeats -eq 0) {
      $BenchmarkRepeats = 1
    }
  }

  "standard" {
    $stressProfile = "medium"
    $providerOnly = ""
    $providerRemoteOnly = "1"
    $realRepos = @(
      "https://github.com/octocat/Hello-World.git",
      "https://github.com/libgit2/TestGitRepository.git"
    )
    if ($BenchmarkRepeats -eq 0) {
      $BenchmarkRepeats = 3
    }
  }

  "exhaustive" {
    $stressProfile = "large"
    $providerOnly = ""
    $providerRemoteOnly = "0"
    $realRepos = @(
      "https://github.com/octocat/Hello-World.git",
      "https://github.com/libgit2/TestGitRepository.git",
      "https://github.com/git/git.git"
    )
    if ($BenchmarkRepeats -eq 0) {
      $BenchmarkRepeats = 5
    }
  }
}

Invoke-BashChecked `
  -Label "Git compatibility stress: $stressProfile" `
  -Command "./tools/git-compat-stress.sh" `
  -Env @{
    ZMIN_BIN = $ZminGitBash
    ZMIN_STRESS_PROFILE = $stressProfile
    RUNNER_OS = "Windows"
    GIT_TERMINAL_PROMPT = "0"
  }

Invoke-BashChecked `
  -Label "Provider smoke" `
  -Command "./tools/git-provider-smoke.sh" `
  -Env @{
    ZMIN_BIN = $ZminGitBash
    ZMIN_PROVIDER_ALLOW_SKIP = "1"
    ZMIN_PROVIDER_ONLY = $providerOnly
    ZMIN_PROVIDER_REMOTE_ONLY = $providerRemoteOnly
    RUNNER_OS = "Windows"
    GIT_TERMINAL_PROMPT = "0"
  }

foreach ($repo in $realRepos) {
  Run-RealRepoSmoke -RepoUrl $repo
}

if (-not $SkipBenchmark) {
  Invoke-Checked `
    -Label "Windows native benchmark" `
    -FilePath "powershell" `
    -Arguments @(
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      (Join-Path $PSScriptRoot "windows-native-benchmark.ps1"),
      "-Repeats",
      "$BenchmarkRepeats"
    )
}

Write-Host "Windows native extended compatibility passed: mode=$Mode"
