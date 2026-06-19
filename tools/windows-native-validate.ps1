param(
  [ValidateSet("targeted", "file", "quick", "integration", "integration-from", "full")]
  [string]$Mode = "targeted",

  [string]$TestFile = "git_cli_failure_compat",

  [string]$Case = "invalid_option_combinations_match_stock_git_failures",

  [int]$TimeoutSeconds = 300,

  [switch]$NoFmt,

  [string]$BuildProfile = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $RepoRoot

$env:GIT_CONFIG_NOSYSTEM = "1"

if (-not $env:CARGO_TARGET_DIR) {
  $env:CARGO_TARGET_DIR = Join-Path $RepoRoot "target"
}
if (-not $BuildProfile) {
  $BuildProfile = if ($env:ZMIN_WINDOWS_VALIDATE_BUILD_PROFILE) { $env:ZMIN_WINDOWS_VALIDATE_BUILD_PROFILE } else { "release" }
}
$BuildDir = Join-Path $env:CARGO_TARGET_DIR $BuildProfile
$ZminExe = Join-Path $BuildDir "zmin.exe"
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
    $resolvedFilePath = (Get-Command $FilePath -ErrorAction Stop).Source
    $processInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $processInfo.FileName = $resolvedFilePath
    $processInfo.WorkingDirectory = (Get-Location).Path
    $processInfo.Arguments = ($Arguments | ForEach-Object {
      if ($_ -match '[\s"]') {
        '"' + ($_ -replace '"', '\"') + '"'
      } else {
        $_
      }
    }) -join " "
    $processInfo.UseShellExecute = $false
    $process = [System.Diagnostics.Process]::Start($processInfo)
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
    "zmin-cli",
    "--test",
    $TestFile,
    $Case,
    "--",
    "--exact",
    "--test-threads=1",
    "--nocapture"
  )
}

function Invoke-TestFile {
  Invoke-Checked -Label "CLI integration test file" -FilePath "cargo" -Arguments @(
    "test",
    "-p",
    "zmin-cli",
    "--test",
    $TestFile,
    "--",
    "--test-threads=1",
    "--nocapture"
  )
}

function Invoke-CliIntegrationSurface {
  param(
    [string]$StartAt = ""
  )

  $testFiles = Get-ChildItem "crates/zmin-cli/tests" -Filter "*.rs" |
    Where-Object { -not $_.Name.StartsWith("._") } |
    ForEach-Object { $_.BaseName } |
    Sort-Object

  foreach ($testFile in $testFiles) {
    if ($StartAt -and ([string]::CompareOrdinal($testFile, $StartAt) -lt 0)) {
      continue
    }

    Write-Host "::group::list tests -p zmin-cli --test $testFile"
    $listOutput = & cargo test -p zmin-cli --test $testFile -- --list
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
        -Label "cargo test -p zmin-cli --test $testFile $caseName" `
        -FilePath "cargo" `
        -Arguments @("test", "-p", "zmin-cli", "--test", $testFile, $caseName, "--", "--exact", "--test-threads=1") `
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

  "file" {
    Invoke-FormatCheck
    Invoke-TestFile
  }

  "quick" {
    Invoke-FormatCheck
    Invoke-Checked -Label "Check CLI binary" -FilePath "cargo" -Arguments @("check", "-p", "zmin-cli", "--bin", "zmin")
    Invoke-TargetedTest
    Invoke-Checked -Label "Build Windows CLI binaries ($BuildProfile)" -FilePath "cargo" -Arguments (Get-CargoBuildArgs -Package "zmin-cli" -ExtraArgs @("--bins"))
    Invoke-Checked -Label "Run zmin.exe" -FilePath $ZminExe -Arguments @("--version")
    Invoke-Checked -Label "Run zmin.exe" -FilePath $ZminGitExe -Arguments @("--version")
  }

  "integration" {
    Invoke-CliIntegrationSurface
  }

  "integration-from" {
    Invoke-CliIntegrationSurface -StartAt $TestFile
  }

  "full" {
    Invoke-FormatCheck
    Invoke-Checked -Label "Check workspace" -FilePath "cargo" -Arguments @("check", "--workspace", "--all-targets")
    Invoke-Checked -Label "Clippy workspace" -FilePath "cargo" -Arguments @("clippy", "--workspace", "--all-targets", "--all-features")
    Invoke-Checked -Label "Test zmin-core" -FilePath "cargo" -Arguments @("test", "-p", "zmin-core", "--all-targets")
    Invoke-Checked -Label "Test zmin-primitives" -FilePath "cargo" -Arguments @("test", "-p", "zmin-primitives", "--all-targets")
    Invoke-Checked -Label "Test zmin-git-core" -FilePath "cargo" -Arguments @("test", "-p", "zmin-git-core", "--all-targets")
    Invoke-Checked -Label "Test zmin-git-remote-http" -FilePath "cargo" -Arguments @("test", "-p", "zmin-git-remote-http", "--all-targets")
    Invoke-Checked -Label "Test CLI unit surface" -FilePath "cargo" -Arguments @("test", "-p", "zmin-cli", "--bin", "zmin")
    Invoke-CliIntegrationSurface
    Invoke-Checked -Label "Build Windows CLI binaries ($BuildProfile)" -FilePath "cargo" -Arguments (Get-CargoBuildArgs -Package "zmin-cli" -ExtraArgs @("--bins"))
    Invoke-Checked -Label "Build Windows remote helper ($BuildProfile)" -FilePath "cargo" -Arguments (Get-CargoBuildArgs -Package "zmin-git-remote-http")
    Invoke-Checked -Label "Run zmin.exe" -FilePath $ZminExe -Arguments @("--version")
    Invoke-Checked -Label "Run zmin.exe" -FilePath $ZminGitExe -Arguments @("--version")
  }
}
