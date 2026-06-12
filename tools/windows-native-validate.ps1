param(
  [ValidateSet("targeted", "quick", "integration", "full")]
  [string]$Mode = "targeted",

  [string]$TestFile = "git_cli_failure_compat",

  [string]$Case = "invalid_option_combinations_match_stock_git_failures",

  [int]$TimeoutSeconds = 300,

  [switch]$NoFmt
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $RepoRoot

$env:GIT_CONFIG_NOSYSTEM = "1"

if (-not $env:CARGO_TARGET_DIR) {
  $env:CARGO_TARGET_DIR = Join-Path $RepoRoot "target"
}
$ReleaseDir = Join-Path $env:CARGO_TARGET_DIR "release"
$SkronExe = Join-Path $ReleaseDir "skron.exe"
$SkronGitExe = Join-Path $ReleaseDir "skron-git.exe"

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

function Invoke-CheckedWithTimeout {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Label,

    [Parameter(Mandatory = $true)]
    [string]$FilePath,

    [string[]]$Arguments = @(),

    [int]$TimeoutSeconds = 300
  )

  Write-Host "::group::$Label"
  Write-Host "+ $FilePath $($Arguments -join ' ')"
  try {
    $process = Start-Process -FilePath $FilePath -ArgumentList $Arguments -NoNewWindow -PassThru
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
      Stop-Process -Id $process.Id -Force
      throw "Timed out after ${TimeoutSeconds}s: $Label"
    }
    if ($process.ExitCode -ne 0) {
      throw "$Label failed with exit code $($process.ExitCode)"
    }
  } finally {
    Write-Host "::endgroup::"
  }
}

function Invoke-FormatCheck {
  if (-not $NoFmt) {
    Invoke-Checked -Label "Format" -FilePath "cargo" -Arguments @("fmt", "--all", "--", "--check")
  }
}

function Invoke-TargetedTest {
  Invoke-Checked -Label "Targeted CLI integration test" -FilePath "cargo" -Arguments @(
    "test",
    "-p",
    "skron-cli",
    "--test",
    $TestFile,
    $Case,
    "--",
    "--exact",
    "--test-threads=1",
    "--nocapture"
  )
}

function Invoke-CliIntegrationSurface {
  $testFiles = Get-ChildItem "crates/skron-cli/tests" -Filter "*.rs" |
    ForEach-Object { $_.BaseName } |
    Sort-Object

  foreach ($testFile in $testFiles) {
    Write-Host "::group::list tests -p skron-cli --test $testFile"
    $listOutput = & cargo test -p skron-cli --test $testFile -- --list
    $listExit = $LASTEXITCODE
    Write-Host "::endgroup::"
    if ($listExit -ne 0) {
      throw "list tests failed for $testFile with exit code $listExit"
    }

    $cases = $listOutput | ForEach-Object {
      if ($_ -match "^([^:]+): test$") {
        $Matches[1]
      }
    }

    foreach ($caseName in $cases) {
      Invoke-CheckedWithTimeout `
        -Label "cargo test -p skron-cli --test $testFile $caseName" `
        -FilePath "cargo" `
        -Arguments @("test", "-p", "skron-cli", "--test", $testFile, $caseName, "--", "--exact", "--test-threads=1") `
        -TimeoutSeconds $TimeoutSeconds
    }
  }
}

Write-Host "Windows native validation mode: $Mode"
Invoke-Checked -Label "Tool versions" -FilePath "cmd" -Arguments @("/c", "rustc --version && cargo --version && git --version")

switch ($Mode) {
  "targeted" {
    Invoke-FormatCheck
    Invoke-TargetedTest
  }

  "quick" {
    Invoke-FormatCheck
    Invoke-Checked -Label "Check CLI binary" -FilePath "cargo" -Arguments @("check", "-p", "skron-cli", "--bin", "skron")
    Invoke-TargetedTest
    Invoke-Checked -Label "Build Windows CLI binaries" -FilePath "cargo" -Arguments @("build", "-p", "skron-cli", "--release", "--bins")
    Invoke-Checked -Label "Run skron.exe" -FilePath $SkronExe -Arguments @("--version")
    Invoke-Checked -Label "Run skron-git.exe" -FilePath $SkronGitExe -Arguments @("--version")
  }

  "integration" {
    Invoke-CliIntegrationSurface
  }

  "full" {
    Invoke-FormatCheck
    Invoke-Checked -Label "Check workspace" -FilePath "cargo" -Arguments @("check", "--workspace", "--all-targets")
    Invoke-Checked -Label "Clippy workspace" -FilePath "cargo" -Arguments @("clippy", "--workspace", "--all-targets", "--all-features")
    Invoke-Checked -Label "Test skron-core" -FilePath "cargo" -Arguments @("test", "-p", "skron-core", "--all-targets")
    Invoke-Checked -Label "Test skron-primitives" -FilePath "cargo" -Arguments @("test", "-p", "skron-primitives", "--all-targets")
    Invoke-Checked -Label "Test skron-git-core" -FilePath "cargo" -Arguments @("test", "-p", "skron-git-core", "--all-targets")
    Invoke-Checked -Label "Test skron-git-remote-http" -FilePath "cargo" -Arguments @("test", "-p", "skron-git-remote-http", "--all-targets")
    Invoke-Checked -Label "Test CLI unit surface" -FilePath "cargo" -Arguments @("test", "-p", "skron-cli", "--bin", "skron")
    Invoke-CliIntegrationSurface
    Invoke-Checked -Label "Build Windows CLI binaries" -FilePath "cargo" -Arguments @("build", "-p", "skron-cli", "--release", "--bins")
    Invoke-Checked -Label "Build Windows remote helper" -FilePath "cargo" -Arguments @("build", "-p", "skron-git-remote-http", "--release")
    Invoke-Checked -Label "Run skron.exe" -FilePath $SkronExe -Arguments @("--version")
    Invoke-Checked -Label "Run skron-git.exe" -FilePath $SkronGitExe -Arguments @("--version")
  }
}
