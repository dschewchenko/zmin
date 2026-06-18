param(
  [int]$Repeats = 5,
  [int]$Commits = 60,
  [int]$FilesPerCommit = 20,
  [int]$WriteFiles = 800,
  [int]$DirtyFiles = 100,
  [int]$PushBatchFiles = 2400,
  [string]$OutDir = "",
  [string]$ZminPhaseTraceDir = "",
  [string]$SshTraceDir = "",
  [string]$SshPacketTraceDir = "",
  [string]$Ops = ""
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
$ZminGitExe = Join-Path $ReleaseDir "zmin.exe"
& cargo build -p zmin-cli --release --bin zmin
if ($LASTEXITCODE -ne 0) {
  throw "failed to build zmin release binary"
}
if (-not (Test-Path -LiteralPath $ZminGitExe)) {
  throw "zmin release binary was not produced at $ZminGitExe"
}

$GitExe = (Get-Command git -ErrorAction Stop).Source
$GixExe = ""
if ($env:GIX_BIN) {
  $GixExe = $env:GIX_BIN
} else {
  $gixCommand = Get-Command gix -ErrorAction SilentlyContinue
  if ($gixCommand) {
    $GixExe = $gixCommand.Source
  }
}
if ($GixExe) {
  Write-Host "gix=$GixExe"
} else {
  Write-Host "gix=not-found (skipping Gitoxide rows)"
}

if (-not $OutDir) {
  $OutDir = Join-Path ([System.IO.Path]::GetTempPath()) ("zmin-windows-bench-" + [Guid]::NewGuid().ToString("N"))
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
if ($ZminPhaseTraceDir) {
  New-Item -ItemType Directory -Force -Path $ZminPhaseTraceDir | Out-Null
}
if ($SshTraceDir) {
  New-Item -ItemType Directory -Force -Path $SshTraceDir | Out-Null
}
if ($SshPacketTraceDir) {
  New-Item -ItemType Directory -Force -Path $SshPacketTraceDir | Out-Null
}

$WorkDir = Join-Path $OutDir "work"
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
$Rows = New-Object System.Collections.Generic.List[object]
$Checks = New-Object System.Collections.Generic.List[object]
$KnownOps = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
@(
  "status",
  "log",
  "rev-list",
  "merge-base",
  "add",
  "commit",
  "add-dirty",
  "commit-dirty",
  "clone",
  "clone-instant",
  "clone-instant-git-daemon",
  "clone-instant-ssh",
  "fetch-noop",
  "fetch-incremental",
  "push-noop",
  "push-incremental",
  "push-batch",
  "pull-noop",
  "pull-incremental"
) | ForEach-Object { [void]$KnownOps.Add($_) }

$SelectedOps = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
if ($Ops.Trim().Length -gt 0) {
  foreach ($op in ($Ops -split '[,;\s]+' | Where-Object { $_.Trim().Length -gt 0 })) {
    if (-not $KnownOps.Contains($op)) {
      throw "unknown benchmark operation '$op'. Known operations: $(@($KnownOps | Sort-Object) -join ', ')"
    }
    [void]$SelectedOps.Add($op)
  }
}

function Test-BenchmarkOp {
  param([string]$Op)

  return $SelectedOps.Count -eq 0 -or $SelectedOps.Contains($Op)
}

function Test-AnyBenchmarkOp {
  param([string[]]$Operations)

  if ($SelectedOps.Count -eq 0) {
    return $true
  }
  foreach ($op in $Operations) {
    if ($SelectedOps.Contains($op)) {
      return $true
    }
  }
  return $false
}

if ($SelectedOps.Count -gt 0) {
  Write-Host "selected_ops=$(@($SelectedOps | Sort-Object) -join ',')"
}

function Invoke-Tool {
  param(
    [Parameter(Mandatory = $true)]
    [string]$FilePath,

    [string[]]$Arguments = @(),

    [string]$WorkingDirectory = (Get-Location).Path,

    [string]$StdOutPath = ""
  )

  $errPath = Join-Path $WorkDir ("stderr-" + [Guid]::NewGuid().ToString("N") + ".txt")
  Push-Location $WorkingDirectory
  $previousErrorActionPreference = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try {
    if ($StdOutPath) {
      & $FilePath @Arguments > $StdOutPath 2> $errPath
    } else {
      & $FilePath @Arguments > $null 2> $errPath
    }
    $exitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousErrorActionPreference
    Pop-Location
  }

  if ($exitCode -ne 0) {
    $stderr = ""
    if (Test-Path -LiteralPath $errPath) {
      $stderr = Get-Content -LiteralPath $errPath -Raw
    }
    throw "$FilePath $($Arguments -join ' ') failed with exit code ${exitCode}: $stderr"
  }
}

function Measure-Tool {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Tool,

    [Parameter(Mandatory = $true)]
    [string]$Op,

    [Parameter(Mandatory = $true)]
    [string]$FilePath,

    [string[]]$Arguments = @(),

    [string]$WorkingDirectory = (Get-Location).Path,

    [string]$Extra = ""
  )

  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $previousPhaseTrace = $env:ZMIN_PHASE_TRACE
  $previousPhaseTraceFile = $env:ZMIN_PHASE_TRACE_FILE
  $previousCheckoutPhaseTrace = $env:ZMIN_CHECKOUT_PHASE_TRACE
  $previousSshTraceFile = $env:ZMIN_BENCH_SSH_TRACE_FILE
  $previousSshTraceTool = $env:ZMIN_BENCH_SSH_TRACE_TOOL
  $previousSshTraceOp = $env:ZMIN_BENCH_SSH_TRACE_OP
  $previousSshTraceExtra = $env:ZMIN_BENCH_SSH_TRACE_EXTRA
  $previousGitTracePacket = $env:GIT_TRACE_PACKET
  if ($Tool -eq "zmin" -and $ZminPhaseTraceDir) {
    $safeExtra = ($Extra -replace '[^A-Za-z0-9_.-]', '_')
    $traceId = [Guid]::NewGuid().ToString("N")
    $traceName = "$Op-$safeExtra-${traceId}.log"
    $env:ZMIN_PHASE_TRACE = "1"
    $env:ZMIN_CHECKOUT_PHASE_TRACE = "1"
    $env:ZMIN_PHASE_TRACE_FILE = Join-Path $ZminPhaseTraceDir $traceName
  }
  if ($Op -eq "clone-instant-ssh" -and $SshTraceDir) {
    $safeExtra = ($Extra -replace '[^A-Za-z0-9_.-]', '_')
    $traceId = [Guid]::NewGuid().ToString("N")
    $traceName = "$Op-$Tool-$safeExtra-${traceId}.tsv"
    $env:ZMIN_BENCH_SSH_TRACE_FILE = ((Join-Path $SshTraceDir $traceName) -replace "\\", "/")
    $env:ZMIN_BENCH_SSH_TRACE_TOOL = $Tool
    $env:ZMIN_BENCH_SSH_TRACE_OP = $Op
    $env:ZMIN_BENCH_SSH_TRACE_EXTRA = $Extra
  }
  if ($Op -eq "clone-instant-ssh" -and $SshPacketTraceDir) {
    $safeExtra = ($Extra -replace '[^A-Za-z0-9_.-]', '_')
    $traceId = [Guid]::NewGuid().ToString("N")
    $traceName = "$Op-$Tool-$safeExtra-${traceId}.packet.log"
    $env:GIT_TRACE_PACKET = ((Join-Path $SshPacketTraceDir $traceName) -replace "\\", "/")
  }
  try {
    Invoke-Tool -FilePath $FilePath -Arguments $Arguments -WorkingDirectory $WorkingDirectory
    $sw.Stop()
  } finally {
    if ($null -eq $previousPhaseTrace) {
      Remove-Item Env:\ZMIN_PHASE_TRACE -ErrorAction SilentlyContinue
    } else {
      $env:ZMIN_PHASE_TRACE = $previousPhaseTrace
    }
    if ($null -eq $previousPhaseTraceFile) {
      Remove-Item Env:\ZMIN_PHASE_TRACE_FILE -ErrorAction SilentlyContinue
    } else {
      $env:ZMIN_PHASE_TRACE_FILE = $previousPhaseTraceFile
    }
    if ($null -eq $previousCheckoutPhaseTrace) {
      Remove-Item Env:\ZMIN_CHECKOUT_PHASE_TRACE -ErrorAction SilentlyContinue
    } else {
      $env:ZMIN_CHECKOUT_PHASE_TRACE = $previousCheckoutPhaseTrace
    }
    if ($null -eq $previousSshTraceFile) {
      Remove-Item Env:\ZMIN_BENCH_SSH_TRACE_FILE -ErrorAction SilentlyContinue
    } else {
      $env:ZMIN_BENCH_SSH_TRACE_FILE = $previousSshTraceFile
    }
    if ($null -eq $previousSshTraceTool) {
      Remove-Item Env:\ZMIN_BENCH_SSH_TRACE_TOOL -ErrorAction SilentlyContinue
    } else {
      $env:ZMIN_BENCH_SSH_TRACE_TOOL = $previousSshTraceTool
    }
    if ($null -eq $previousSshTraceOp) {
      Remove-Item Env:\ZMIN_BENCH_SSH_TRACE_OP -ErrorAction SilentlyContinue
    } else {
      $env:ZMIN_BENCH_SSH_TRACE_OP = $previousSshTraceOp
    }
    if ($null -eq $previousSshTraceExtra) {
      Remove-Item Env:\ZMIN_BENCH_SSH_TRACE_EXTRA -ErrorAction SilentlyContinue
    } else {
      $env:ZMIN_BENCH_SSH_TRACE_EXTRA = $previousSshTraceExtra
    }
    if ($null -eq $previousGitTracePacket) {
      Remove-Item Env:\GIT_TRACE_PACKET -ErrorAction SilentlyContinue
    } else {
      $env:GIT_TRACE_PACKET = $previousGitTracePacket
    }
  }

  $Rows.Add([pscustomobject]@{
    tool = $Tool
    op = $Op
    seconds = [Math]::Round($sw.Elapsed.TotalSeconds, 6)
    exit = 0
    extra = $Extra
  }) | Out-Null
}

function Add-Check {
  param(
    [string]$Name,
    [string]$Status,
    [string]$Details
  )

  $Checks.Add([pscustomobject]@{
    check = $Name
    status = $Status
    details = $Details
  }) | Out-Null
}

function Assert-SameFile {
  param(
    [string]$Name,
    [string]$Left,
    [string]$Right
  )

  $leftHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Left).Hash
  $rightHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Right).Hash
  if ($leftHash -ne $rightHash) {
    Add-Check -Name $Name -Status "fail" -Details "sha256 mismatch"
    throw "$Name output mismatch"
  }
  Add-Check -Name $Name -Status "ok" -Details $leftHash
}

function Assert-SameRef {
  param(
    [string]$Name,
    [string]$LeftRepo,
    [string]$RightRepo,
    [string]$Ref
  )

  $left = & $GitExe -C $LeftRepo rev-parse $Ref
  $right = & $GitExe -C $RightRepo rev-parse $Ref
  if ($LASTEXITCODE -ne 0 -or $left -ne $right) {
    Add-Check -Name $Name -Status "fail" -Details "$Ref mismatch"
    throw "$Name ref mismatch"
  }
  Add-Check -Name $Name -Status "ok" -Details "$Ref=$left"
}

function Assert-ConfigValue {
  param(
    [string]$Name,
    [string]$Repo,
    [string]$Key,
    [string]$Expected
  )

  $value = & $GitExe -C $Repo config --get $Key
  if ($LASTEXITCODE -ne 0 -or $value -ne $Expected) {
    Add-Check -Name $Name -Status "fail" -Details "$Key mismatch"
    throw "$Name config mismatch"
  }
  Add-Check -Name $Name -Status "ok" -Details "$Key=$Expected"
}

function Get-FreeTcpPort {
  $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  $listener.Start()
  try {
    return $listener.LocalEndpoint.Port
  } finally {
    $listener.Stop()
  }
}

function New-FakeSshScript {
  param([string]$Root)

  $script = Join-Path $Root "fake-ssh.sh"
  @'
#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in
    -p|-l|-o|-F|-i|-J)
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -*)
      shift
      ;;
    *)
      break
      ;;
  esac
done
if [ "$#" -lt 2 ]; then
  echo "fake ssh missing remote command" >&2
  exit 1
fi
shift
cmd="$*"
cmd="$(printf '%s\n' "$cmd" | sed -E "s#'/(.):#'\1:#g; s#\"/(.):#\"\1:#g; s# /(.:)# \1#g")"
if [ "${ZMIN_BENCH_FAKE_SSH_GIT_EXEC_PATH:-}" ]; then
  PATH="$ZMIN_BENCH_FAKE_SSH_GIT_EXEC_PATH:$PATH"
  export PATH
fi
if [ "${ZMIN_BENCH_SSH_TRACE_FILE:-}" ]; then
  trace_file="$ZMIN_BENCH_SSH_TRACE_FILE"
  if [ ! -s "$trace_file" ]; then
    printf 'tool\top\textra\tgit_protocol\tstart_ns\tend_ns\treal_seconds\texit\tcommand\n' >"$trace_file"
  fi
  start_ns="$(date +%s%N 2>/dev/null || date +%s000000000)"
  set +e
  /bin/sh -c "$cmd"
  status=$?
  set -e
  end_ns="$(date +%s%N 2>/dev/null || date +%s000000000)"
  real_seconds="$(awk -v start="$start_ns" -v end="$end_ns" 'BEGIN { printf "%.6f", (end - start) / 1000000000 }')"
  safe_cmd="$(printf '%s' "$cmd" | tr '\t\r\n' '   ')"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${ZMIN_BENCH_SSH_TRACE_TOOL:-}" \
    "${ZMIN_BENCH_SSH_TRACE_OP:-}" \
    "${ZMIN_BENCH_SSH_TRACE_EXTRA:-}" \
    "${GIT_PROTOCOL:-}" \
    "$start_ns" \
    "$end_ns" \
    "$real_seconds" \
    "$status" \
    "$safe_cmd" >>"$trace_file"
  exit "$status"
fi
exec /bin/sh -c "$cmd"
'@ | Set-Content -LiteralPath $script -Encoding ASCII
  $gitRoot = Split-Path (Split-Path $GitExe -Parent) -Parent
  $shell = Join-Path $gitRoot "bin\sh.exe"
  if (-not (Test-Path -LiteralPath $shell)) {
    $shell = Join-Path $gitRoot "usr\bin\sh.exe"
  }
  if (-not (Test-Path -LiteralPath $shell)) {
    throw "Git shell not found for fake SSH"
  }
  return (($shell -replace "\\", "/") + " " + ($script -replace "\\", "/"))
}

function Invoke-WithGitSshCommand {
  param(
    [string]$Command,
    [scriptblock]$Script
  )

  $previous = $env:GIT_SSH_COMMAND
  $previousFakeSshGitExecPath = $env:ZMIN_BENCH_FAKE_SSH_GIT_EXEC_PATH
  $env:GIT_SSH_COMMAND = $Command
  $env:ZMIN_BENCH_FAKE_SSH_GIT_EXEC_PATH = & $GitExe --exec-path
  try {
    & $Script
  } finally {
    if ($null -eq $previous) {
      Remove-Item Env:\GIT_SSH_COMMAND -ErrorAction SilentlyContinue
    } else {
      $env:GIT_SSH_COMMAND = $previous
    }
    if ($null -eq $previousFakeSshGitExecPath) {
      Remove-Item Env:\ZMIN_BENCH_FAKE_SSH_GIT_EXEC_PATH -ErrorAction SilentlyContinue
    } else {
      $env:ZMIN_BENCH_FAKE_SSH_GIT_EXEC_PATH = $previousFakeSshGitExecPath
    }
  }
}

function Start-BenchmarkGitDaemon {
  param(
    [string]$BasePath,
    [string]$Url
  )

  $port = [int](($Url -split ":")[2] -split "/")[0]
  $stdout = Join-Path $WorkDir "git-daemon.stdout"
  $stderr = Join-Path $WorkDir "git-daemon.stderr"
  $args = @(
    "daemon",
    "--reuseaddr",
    "--base-path=$BasePath",
    "--export-all",
    "--listen=127.0.0.1",
    "--port=$port",
    $BasePath
  )
  $process = Start-Process -FilePath $GitExe -ArgumentList $args -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
  for ($attempt = 0; $attempt -lt 100; $attempt++) {
    try {
      Invoke-Tool -FilePath $GitExe -Arguments @("ls-remote", $Url, "HEAD")
      return $process
    } catch {
      Start-Sleep -Milliseconds 100
    }
  }
  if (Test-Path -LiteralPath $stderr) {
    Get-Content -LiteralPath $stderr -Raw | Write-Error
  }
  throw "git daemon did not become ready"
}

function Stop-BenchmarkGitDaemon {
  param(
    [object]$Process,
    [string]$BasePath
  )

  if ($Process -and -not $Process.HasExited) {
    Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
  }

  $escapedBasePath = [regex]::Escape($BasePath)
  Get-CimInstance Win32_Process |
    Where-Object { $_.CommandLine -match "daemon" -and $_.CommandLine -match $escapedBasePath } |
    ForEach-Object {
      Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
    }
}

function Get-Median {
  param([double[]]$Values)

  if ($Values.Count -eq 0) {
    return 0.0
  }

  $sorted = @($Values | Sort-Object)
  $middle = [int]($sorted.Count / 2)
  if (($sorted.Count % 2) -eq 1) {
    return $sorted[$middle]
  }
  return ($sorted[$middle - 1] + $sorted[$middle]) / 2.0
}

function Get-Ratio {
  param(
    [double]$Numerator,
    [double]$Denominator
  )

  if ($Denominator -eq 0.0) {
    return 0.0
  }
  return [Math]::Round($Numerator / $Denominator, 6)
}

function Get-PairedRatios {
  param(
    [object[]]$Rows,
    [string]$Op,
    [string]$NumeratorTool,
    [string]$DenominatorTool
  )

  $numerators = @{}
  $denominators = @{}
  foreach ($row in $Rows) {
    if ($row.op -ne $Op) {
      continue
    }
    if ($row.tool -eq $NumeratorTool) {
      $numerators[$row.extra] = [double]$row.seconds
    } elseif ($row.tool -eq $DenominatorTool) {
      $denominators[$row.extra] = [double]$row.seconds
    }
  }

  $ratios = New-Object System.Collections.Generic.List[double]
  foreach ($extra in $numerators.Keys) {
    if (-not $denominators.ContainsKey($extra)) {
      continue
    }
    $denominator = [double]$denominators[$extra]
    if ($denominator -eq 0.0) {
      continue
    }
    $ratios.Add(([double]$numerators[$extra]) / $denominator)
  }
  return @($ratios | Sort-Object)
}

function Configure-Repo {
  param([string]$Path)
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Path, "config", "user.name", "Bench")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Path, "config", "user.email", "bench@example.test")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Path, "config", "commit.gpgsign", "false")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Path, "config", "core.autocrlf", "false")
}

function Write-Files {
  param(
    [string]$Root,
    [int]$Count,
    [string]$Prefix
  )

  for ($i = 1; $i -le $Count; $i++) {
    $dir = Join-Path $Root ("dir-" + ($i % 32))
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $path = Join-Path $dir ("file-$i.txt")
    "prefix=$Prefix file=$i payload=$('0' * 1024)" | Set-Content -LiteralPath $path -Encoding UTF8
  }
}

function Invoke-Both {
  param(
    [string]$Op,
    [string[]]$GitArgs,
    [string[]]$ZminArgs,
    [string]$GitWorkingDirectory = (Get-Location).Path,
    [string]$ZminWorkingDirectory = (Get-Location).Path,
    [string]$Extra = ""
  )

  for ($n = 1; $n -le $Repeats; $n++) {
    if (($n % 2) -eq 0) {
      Measure-Tool -Tool "zmin" -Op $Op -FilePath $ZminGitExe -Arguments $ZminArgs -WorkingDirectory $ZminWorkingDirectory -Extra "$n/$Extra"
      Measure-Tool -Tool "git" -Op $Op -FilePath $GitExe -Arguments $GitArgs -WorkingDirectory $GitWorkingDirectory -Extra "$n/$Extra"
    } else {
      Measure-Tool -Tool "git" -Op $Op -FilePath $GitExe -Arguments $GitArgs -WorkingDirectory $GitWorkingDirectory -Extra "$n/$Extra"
      Measure-Tool -Tool "zmin" -Op $Op -FilePath $ZminGitExe -Arguments $ZminArgs -WorkingDirectory $ZminWorkingDirectory -Extra "$n/$Extra"
    }
  }
}

function Invoke-GixRepeated {
  param(
    [string]$Op,
    [string[]]$Arguments,
    [string]$WorkingDirectory = (Get-Location).Path,
    [string]$Extra = ""
  )

  if (-not $GixExe) {
    return
  }

  for ($n = 1; $n -le $Repeats; $n++) {
    Measure-Tool -Tool "gix" -Op $Op -FilePath $GixExe -Arguments $Arguments -WorkingDirectory $WorkingDirectory -Extra "$n/$Extra"
  }
}

$Src = Join-Path $WorkDir "src"
Invoke-Tool -FilePath $GitExe -Arguments @("init", "-q", "-b", "main", $Src)
Configure-Repo -Path $Src

for ($c = 1; $c -le $Commits; $c++) {
  $dir = Join-Path $Src ("dir-" + ($c % 24))
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  for ($f = 1; $f -le $FilesPerCommit; $f++) {
    "commit=$c file=$f payload=$('0' * 1024)" | Set-Content -LiteralPath (Join-Path $dir "file-$f.txt") -Encoding UTF8
  }
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "add", "-A")
  $env:GIT_AUTHOR_DATE = (1700000000 + $c).ToString() + " +0000"
  $env:GIT_COMMITTER_DATE = $env:GIT_AUTHOR_DATE
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "commit", "-qm", "commit $c")
}
Remove-Item Env:\GIT_AUTHOR_DATE -ErrorAction SilentlyContinue
Remove-Item Env:\GIT_COMMITTER_DATE -ErrorAction SilentlyContinue
Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "repack", "-adq")
Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "fsck", "--strict")

if (Test-BenchmarkOp "status") {
  $gitStatus = Join-Path $WorkDir "git-status.txt"
  $zminStatus = Join-Path $WorkDir "zmin-status.txt"
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "status", "--porcelain=v1", "--branch") -StdOutPath $gitStatus
  Invoke-Tool -FilePath $ZminGitExe -Arguments @("-C", $Src, "status", "--porcelain=v1", "--branch") -StdOutPath $zminStatus
  Assert-SameFile -Name "status-output" -Left $gitStatus -Right $zminStatus
  Invoke-Both -Op "status" -GitArgs @("-C", $Src, "status", "--porcelain=v1", "--branch") -ZminArgs @("-C", $Src, "status", "--porcelain=v1", "--branch") -Extra "clean"
  Invoke-GixRepeated -Op "status" -Arguments @("-r", $Src, "status", "--format", "simplified") -Extra "clean"
}

if (Test-BenchmarkOp "log") {
  $gitLog = Join-Path $WorkDir "git-log.txt"
  $zminLog = Join-Path $WorkDir "zmin-log.txt"
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "log", "--oneline", "--max-count", "$Commits") -StdOutPath $gitLog
  Invoke-Tool -FilePath $ZminGitExe -Arguments @("-C", $Src, "log", "--oneline", "--max-count", "$Commits") -StdOutPath $zminLog
  Assert-SameFile -Name "log-output" -Left $gitLog -Right $zminLog
  Invoke-Both -Op "log" -GitArgs @("-C", $Src, "log", "--oneline", "--max-count", "$Commits") -ZminArgs @("-C", $Src, "log", "--oneline", "--max-count", "$Commits") -Extra "$Commits commits"
  Invoke-GixRepeated -Op "log" -Arguments @("-r", $Src, "log") -Extra "$Commits commits"
}

if (Test-BenchmarkOp "rev-list") {
  $gitRevList = Join-Path $WorkDir "git-rev-list.txt"
  $zminRevList = Join-Path $WorkDir "zmin-rev-list.txt"
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "rev-list", "--objects", "--all") -StdOutPath $gitRevList
  Invoke-Tool -FilePath $ZminGitExe -Arguments @("-C", $Src, "rev-list", "--objects", "--all") -StdOutPath $zminRevList
  Assert-SameFile -Name "rev-list-output" -Left $gitRevList -Right $zminRevList
  Invoke-Both -Op "rev-list" -GitArgs @("-C", $Src, "rev-list", "--objects", "--all") -ZminArgs @("-C", $Src, "rev-list", "--objects", "--all") -Extra "all"
}

if (Test-BenchmarkOp "merge-base") {
  Invoke-Both -Op "merge-base" -GitArgs @("-C", $Src, "merge-base", "HEAD", "HEAD~$([Math]::Max(1, [int]($Commits / 2)))") -ZminArgs @("-C", $Src, "merge-base", "HEAD", "HEAD~$([Math]::Max(1, [int]($Commits / 2)))") -Extra "mid"
  Invoke-GixRepeated -Op "merge-base" -Arguments @("-r", $Src, "merge-base", "HEAD", "HEAD~$([Math]::Max(1, [int]($Commits / 2)))") -Extra "mid"
}

if (Test-AnyBenchmarkOp @("add", "commit", "add-dirty", "commit-dirty")) {
  for ($n = 1; $n -le $Repeats; $n++) {
    $gitRepo = Join-Path $WorkDir "git-write-$n"
    $zminRepo = Join-Path $WorkDir "zmin-write-$n"
    Invoke-Tool -FilePath $GitExe -Arguments @("init", "-q", "-b", "main", $gitRepo)
    Invoke-Tool -FilePath $ZminGitExe -Arguments @("init", $zminRepo)
    Configure-Repo -Path $gitRepo
    Configure-Repo -Path $zminRepo
    Write-Files -Root $gitRepo -Count $WriteFiles -Prefix "initial"
    Write-Files -Root $zminRepo -Count $WriteFiles -Prefix "initial"

    if (Test-BenchmarkOp "add") {
      Measure-Tool -Tool "git" -Op "add" -FilePath $GitExe -Arguments @("-C", $gitRepo, "add", "-A") -Extra "$n/$WriteFiles files"
      Measure-Tool -Tool "zmin" -Op "add" -FilePath $ZminGitExe -Arguments @("-C", $zminRepo, "add", "-A") -Extra "$n/$WriteFiles files"
    } else {
      Invoke-Tool -FilePath $GitExe -Arguments @("-C", $gitRepo, "add", "-A")
      Invoke-Tool -FilePath $ZminGitExe -Arguments @("-C", $zminRepo, "add", "-A")
    }

    $env:GIT_AUTHOR_DATE = "1700000000 +0000"
    $env:GIT_COMMITTER_DATE = "1700000000 +0000"
    if (Test-BenchmarkOp "commit") {
      Measure-Tool -Tool "git" -Op "commit" -FilePath $GitExe -Arguments @("-C", $gitRepo, "commit", "-qm", "initial") -Extra "$n/$WriteFiles files"
      Measure-Tool -Tool "zmin" -Op "commit" -FilePath $ZminGitExe -Arguments @("-C", $zminRepo, "commit", "-qm", "initial") -Extra "$n/$WriteFiles files"
    } else {
      Invoke-Tool -FilePath $GitExe -Arguments @("-C", $gitRepo, "commit", "-qm", "initial")
      Invoke-Tool -FilePath $ZminGitExe -Arguments @("-C", $zminRepo, "commit", "-qm", "initial")
    }

    if (Test-AnyBenchmarkOp @("add-dirty", "commit-dirty")) {
      for ($i = 1; $i -le $DirtyFiles; $i++) {
        Add-Content -LiteralPath (Join-Path $gitRepo ("dir-" + ($i % 32) + "\file-$i.txt")) -Value "changed $i"
        Add-Content -LiteralPath (Join-Path $zminRepo ("dir-" + ($i % 32) + "\file-$i.txt")) -Value "changed $i"
      }

      if (Test-BenchmarkOp "add-dirty") {
        Measure-Tool -Tool "git" -Op "add-dirty" -FilePath $GitExe -Arguments @("-C", $gitRepo, "add", "-A") -Extra "$n/$DirtyFiles files"
        Measure-Tool -Tool "zmin" -Op "add-dirty" -FilePath $ZminGitExe -Arguments @("-C", $zminRepo, "add", "-A") -Extra "$n/$DirtyFiles files"
      } else {
        Invoke-Tool -FilePath $GitExe -Arguments @("-C", $gitRepo, "add", "-A")
        Invoke-Tool -FilePath $ZminGitExe -Arguments @("-C", $zminRepo, "add", "-A")
      }

      if (Test-BenchmarkOp "commit-dirty") {
        $env:GIT_AUTHOR_DATE = "1700000001 +0000"
        $env:GIT_COMMITTER_DATE = "1700000001 +0000"
        Measure-Tool -Tool "git" -Op "commit-dirty" -FilePath $GitExe -Arguments @("-C", $gitRepo, "commit", "-qm", "dirty") -Extra "$n/$DirtyFiles files"
        Measure-Tool -Tool "zmin" -Op "commit-dirty" -FilePath $ZminGitExe -Arguments @("-C", $zminRepo, "commit", "-qm", "dirty") -Extra "$n/$DirtyFiles files"
        Assert-SameRef -Name "commit-dirty-$n" -LeftRepo $gitRepo -RightRepo $zminRepo -Ref "HEAD^{tree}"
      }
    }
  }
}
Remove-Item Env:\GIT_AUTHOR_DATE -ErrorAction SilentlyContinue
Remove-Item Env:\GIT_COMMITTER_DATE -ErrorAction SilentlyContinue

$DaemonProcess = $null
if (Test-BenchmarkOp "clone-instant-git-daemon") {
  $DaemonRemote = Join-Path $WorkDir "daemon-remote.git"
  Invoke-Tool -FilePath $GitExe -Arguments @("clone", "-q", "--bare", $Src, $DaemonRemote)
  Invoke-Tool -FilePath $GitExe -Arguments @("--git-dir", $DaemonRemote, "symbolic-ref", "HEAD", "refs/heads/main")
  New-Item -ItemType File -Force -Path (Join-Path $DaemonRemote "git-daemon-export-ok") | Out-Null
  $DaemonPort = Get-FreeTcpPort
  $DaemonUrl = "git://127.0.0.1:$DaemonPort/daemon-remote.git"
  $DaemonProcess = Start-BenchmarkGitDaemon -BasePath $WorkDir -Url $DaemonUrl
}

if (Test-BenchmarkOp "clone-instant-ssh") {
  $SshRemote = Join-Path $WorkDir "ssh-remote.git"
  Invoke-Tool -FilePath $GitExe -Arguments @("clone", "-q", "--bare", $Src, $SshRemote)
  Invoke-Tool -FilePath $GitExe -Arguments @("--git-dir", $SshRemote, "symbolic-ref", "HEAD", "refs/heads/main")
  $FakeSshCommand = New-FakeSshScript -Root $WorkDir
  $SshUrl = "ssh://example.test/" + ($SshRemote -replace "\\", "/")
}

try {
  if (Test-AnyBenchmarkOp @("clone", "clone-instant", "clone-instant-git-daemon", "clone-instant-ssh")) {
    for ($n = 1; $n -le $Repeats; $n++) {
      if (Test-BenchmarkOp "clone") {
        $gitClone = Join-Path $WorkDir "git-clone-$n"
        $zminClone = Join-Path $WorkDir "zmin-clone-$n"
        Measure-Tool -Tool "git" -Op "clone" -FilePath $GitExe -Arguments @("clone", "-q", $Src, $gitClone) -Extra "$n/local"
        if ($GixExe) {
          $gixClone = Join-Path $WorkDir "gix-clone-$n"
          Measure-Tool -Tool "gix" -Op "clone" -FilePath $GixExe -Arguments @("clone", $Src, $gixClone) -Extra "$n/local"
        }
        Measure-Tool -Tool "zmin" -Op "clone" -FilePath $ZminGitExe -Arguments @("clone", "-q", $Src, $zminClone) -Extra "$n/local"
        Assert-SameRef -Name "clone-$n" -LeftRepo $gitClone -RightRepo $zminClone -Ref "HEAD"
        Assert-SameRef -Name "clone-$n-tree" -LeftRepo $gitClone -RightRepo $zminClone -Ref "HEAD^{tree}"
      }

      if (Test-BenchmarkOp "clone-instant") {
        $gitInstantClone = Join-Path $WorkDir "git-clone-instant-$n"
        $zminInstantClone = Join-Path $WorkDir "zmin-clone-instant-$n"
        Measure-Tool -Tool "git" -Op "clone-instant" -FilePath $GitExe -Arguments @("clone", "-q", $Src, $gitInstantClone) -Extra "$n/local"
        Measure-Tool -Tool "zmin" -Op "clone-instant" -FilePath $ZminGitExe -Arguments @("clone", "-q", "--instant", $Src, $zminInstantClone) -Extra "$n/local"
        Assert-SameRef -Name "clone-instant-$n" -LeftRepo $gitInstantClone -RightRepo $zminInstantClone -Ref "HEAD"
        Assert-SameRef -Name "clone-instant-$n-tree" -LeftRepo $gitInstantClone -RightRepo $zminInstantClone -Ref "HEAD^{tree}"
        Assert-ConfigValue -Name "clone-instant-$n-marker" -Repo $zminInstantClone -Key "zmin.worktreeFirst" -Expected "true"
      }

      if (Test-BenchmarkOp "clone-instant-git-daemon") {
        $gitDaemonInstantClone = Join-Path $WorkDir "git-daemon-instant-baseline-$n"
        $zminDaemonInstantClone = Join-Path $WorkDir "zmin-daemon-instant-$n"
        Measure-Tool -Tool "git" -Op "clone-instant-git-daemon" -FilePath $GitExe -Arguments @("clone", "-q", $DaemonUrl, $gitDaemonInstantClone) -Extra "$n/git-daemon"
        Measure-Tool -Tool "zmin" -Op "clone-instant-git-daemon" -FilePath $ZminGitExe -Arguments @("clone", "-q", "--instant", $DaemonUrl, $zminDaemonInstantClone) -Extra "$n/git-daemon"
        Invoke-Tool -FilePath $GitExe -Arguments @("-C", $zminDaemonInstantClone, "fsck", "--strict")
        Assert-SameRef -Name "clone-instant-git-daemon-$n" -LeftRepo $gitDaemonInstantClone -RightRepo $zminDaemonInstantClone -Ref "HEAD"
        Assert-SameRef -Name "clone-instant-git-daemon-$n-tree" -LeftRepo $gitDaemonInstantClone -RightRepo $zminDaemonInstantClone -Ref "HEAD^{tree}"
        Assert-ConfigValue -Name "clone-instant-git-daemon-$n-marker" -Repo $zminDaemonInstantClone -Key "zmin.worktreeFirst" -Expected "true"
      }

      if (Test-BenchmarkOp "clone-instant-ssh") {
        $gitSshInstantClone = Join-Path $WorkDir "git-ssh-instant-baseline-$n"
        $zminSshInstantClone = Join-Path $WorkDir "zmin-ssh-instant-$n"
        Invoke-WithGitSshCommand -Command $FakeSshCommand -Script {
          Measure-Tool -Tool "git" -Op "clone-instant-ssh" -FilePath $GitExe -Arguments @("clone", "-q", $SshUrl, $gitSshInstantClone) -Extra "$n/ssh"
        }
        Invoke-WithGitSshCommand -Command $FakeSshCommand -Script {
          Measure-Tool -Tool "zmin" -Op "clone-instant-ssh" -FilePath $ZminGitExe -Arguments @("clone", "-q", "--instant", $SshUrl, $zminSshInstantClone) -Extra "$n/ssh"
        }
        Invoke-Tool -FilePath $GitExe -Arguments @("-C", $zminSshInstantClone, "fsck", "--strict")
        Assert-SameRef -Name "clone-instant-ssh-$n" -LeftRepo $gitSshInstantClone -RightRepo $zminSshInstantClone -Ref "HEAD"
        Assert-SameRef -Name "clone-instant-ssh-$n-tree" -LeftRepo $gitSshInstantClone -RightRepo $zminSshInstantClone -Ref "HEAD^{tree}"
        Assert-ConfigValue -Name "clone-instant-ssh-$n-marker" -Repo $zminSshInstantClone -Key "zmin.worktreeFirst" -Expected "true"
      }
    }
  }
} finally {
  Stop-BenchmarkGitDaemon -Process $DaemonProcess -BasePath $WorkDir
}

if (Test-AnyBenchmarkOp @("push-noop", "push-incremental", "push-batch")) {
  $PushRemote = Join-Path $WorkDir "push-remote.git"
  Invoke-Tool -FilePath $GitExe -Arguments @("init", "-q", "--bare", $PushRemote)
  Invoke-Tool -FilePath $GitExe -Arguments @("clone", "-q", $Src, (Join-Path $WorkDir "git-push-base"))
  Invoke-Tool -FilePath $ZminGitExe -Arguments @("clone", "-q", $Src, (Join-Path $WorkDir "zmin-push-base"))
  Configure-Repo -Path (Join-Path $WorkDir "git-push-base")
  Configure-Repo -Path (Join-Path $WorkDir "zmin-push-base")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "git-push-base"), "remote", "remove", "origin")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "zmin-push-base"), "remote", "remove", "origin")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "git-push-base"), "remote", "add", "origin", $PushRemote)
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "zmin-push-base"), "remote", "add", "origin", $PushRemote)
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "git-push-base"), "push", "-q", "origin", "main")
  Invoke-Tool -FilePath $GitExe -Arguments @("--git-dir", $PushRemote, "symbolic-ref", "HEAD", "refs/heads/main")

  if (Test-BenchmarkOp "push-noop") {
    Invoke-Both `
      -Op "push-noop" `
      -GitArgs @("-C", (Join-Path $WorkDir "git-push-base"), "push", "origin", "main") `
      -ZminArgs @("-C", (Join-Path $WorkDir "zmin-push-base"), "push", "origin", "main") `
      -Extra "remote"
    Assert-SameRef -Name "push-noop" -LeftRepo (Join-Path $WorkDir "git-push-base") -RightRepo (Join-Path $WorkDir "zmin-push-base") -Ref "HEAD"
  }

  if (Test-BenchmarkOp "push-incremental") {
    "incremental" | Set-Content -LiteralPath (Join-Path $WorkDir "git-push-base\incremental.txt") -Encoding UTF8
    "incremental" | Set-Content -LiteralPath (Join-Path $WorkDir "zmin-push-base\incremental.txt") -Encoding UTF8
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "git-push-base"), "add", "-A")
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "zmin-push-base"), "add", "-A")
    $env:GIT_AUTHOR_DATE = "1700080000 +0000"
    $env:GIT_COMMITTER_DATE = "1700080000 +0000"
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "git-push-base"), "commit", "-qm", "incremental")
    Invoke-Tool -FilePath $ZminGitExe -Arguments @("-C", (Join-Path $WorkDir "zmin-push-base"), "commit", "-qm", "incremental")
    Remove-Item Env:\GIT_AUTHOR_DATE -ErrorAction SilentlyContinue
    Remove-Item Env:\GIT_COMMITTER_DATE -ErrorAction SilentlyContinue
    Assert-SameRef -Name "push-incremental-prep-tree" -LeftRepo (Join-Path $WorkDir "git-push-base") -RightRepo (Join-Path $WorkDir "zmin-push-base") -Ref "HEAD^{tree}"

    for ($n = 1; $n -le $Repeats; $n++) {
      Measure-Tool -Tool "git" -Op "push-incremental" -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "git-push-base"), "push", "origin", "HEAD:refs/heads/git-incremental-$n") -Extra "$n/remote"
      Measure-Tool -Tool "zmin" -Op "push-incremental" -FilePath $ZminGitExe -Arguments @("-C", (Join-Path $WorkDir "zmin-push-base"), "push", "origin", "HEAD:refs/heads/zmin-incremental-$n") -Extra "$n/remote"
      Invoke-Tool -FilePath $GitExe -Arguments @("--git-dir", $PushRemote, "rev-parse", "refs/heads/git-incremental-$n")
      Invoke-Tool -FilePath $GitExe -Arguments @("--git-dir", $PushRemote, "rev-parse", "refs/heads/zmin-incremental-$n")
    }
    Add-Check -Name "push-incremental" -Status "ok" -Details "refs_present"
  }

  if (Test-BenchmarkOp "push-batch") {
    $PushBatchRemote = Join-Path $WorkDir "push-batch-remote.git"
    Invoke-Tool -FilePath $GitExe -Arguments @("init", "-q", "--bare", $PushBatchRemote)
    Invoke-Tool -FilePath $GitExe -Arguments @("clone", "-q", $Src, (Join-Path $WorkDir "git-push-batch-base"))
    Invoke-Tool -FilePath $ZminGitExe -Arguments @("clone", "-q", $Src, (Join-Path $WorkDir "zmin-push-batch-base"))
    Configure-Repo -Path (Join-Path $WorkDir "git-push-batch-base")
    Configure-Repo -Path (Join-Path $WorkDir "zmin-push-batch-base")
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "git-push-batch-base"), "remote", "remove", "origin")
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "zmin-push-batch-base"), "remote", "remove", "origin")
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "git-push-batch-base"), "remote", "add", "origin", $PushBatchRemote)
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "zmin-push-batch-base"), "remote", "add", "origin", $PushBatchRemote)
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", (Join-Path $WorkDir "git-push-batch-base"), "push", "-q", "origin", "main")

    for ($n = 1; $n -le $Repeats; $n++) {
      $gitPushBatch = Join-Path $WorkDir "git-push-batch-$n"
      $zminPushBatch = Join-Path $WorkDir "zmin-push-batch-$n"
      Copy-Item -Recurse -LiteralPath (Join-Path $WorkDir "git-push-batch-base") -Destination $gitPushBatch
      Copy-Item -Recurse -LiteralPath (Join-Path $WorkDir "zmin-push-batch-base") -Destination $zminPushBatch
      New-Item -ItemType Directory -Force -Path (Join-Path $gitPushBatch "push-batch") | Out-Null
      New-Item -ItemType Directory -Force -Path (Join-Path $zminPushBatch "push-batch") | Out-Null
      for ($i = 1; $i -le $PushBatchFiles; $i++) {
        "push batch $i $('0' * 4096)" | Set-Content -LiteralPath (Join-Path $gitPushBatch "push-batch\file-$i.txt") -Encoding UTF8
        "push batch $i $('0' * 4096)" | Set-Content -LiteralPath (Join-Path $zminPushBatch "push-batch\file-$i.txt") -Encoding UTF8
      }
      Invoke-Tool -FilePath $GitExe -Arguments @("-C", $gitPushBatch, "add", "-A")
      Invoke-Tool -FilePath $GitExe -Arguments @("-C", $zminPushBatch, "add", "-A")
      $timestamp = 1700081000 + $n
      $env:GIT_AUTHOR_DATE = "$timestamp +0000"
      $env:GIT_COMMITTER_DATE = "$timestamp +0000"
      Invoke-Tool -FilePath $GitExe -Arguments @("-C", $gitPushBatch, "commit", "-qm", "push-batch")
      Invoke-Tool -FilePath $ZminGitExe -Arguments @("-C", $zminPushBatch, "commit", "-qm", "push-batch")
      Remove-Item Env:\GIT_AUTHOR_DATE -ErrorAction SilentlyContinue
      Remove-Item Env:\GIT_COMMITTER_DATE -ErrorAction SilentlyContinue
      Assert-SameRef -Name "push-batch-prep-$n" -LeftRepo $gitPushBatch -RightRepo $zminPushBatch -Ref "HEAD^{tree}"
      Measure-Tool -Tool "git" -Op "push-batch" -FilePath $GitExe -Arguments @("-C", $gitPushBatch, "push", "origin", "HEAD:refs/heads/git-push-batch-$n") -Extra "$n/$PushBatchFiles files"
      Measure-Tool -Tool "zmin" -Op "push-batch" -FilePath $ZminGitExe -Arguments @("-C", $zminPushBatch, "push", "origin", "HEAD:refs/heads/zmin-push-batch-$n") -Extra "$n/$PushBatchFiles files"
    }
    Add-Check -Name "push-batch" -Status "ok" -Details "refs_pushed"
  }
}

if (Test-AnyBenchmarkOp @("pull-noop", "pull-incremental")) {
  $PullRemote = Join-Path $WorkDir "pull-remote.git"
  $PullSource = Join-Path $WorkDir "pull-source"
  Invoke-Tool -FilePath $GitExe -Arguments @("init", "-q", "--bare", $PullRemote)
  Invoke-Tool -FilePath $GitExe -Arguments @("clone", "-q", $Src, $PullSource)
  Configure-Repo -Path $PullSource
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $PullSource, "remote", "remove", "origin")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $PullSource, "remote", "add", "origin", $PullRemote)
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $PullSource, "push", "-q", "origin", "main")
  Invoke-Tool -FilePath $GitExe -Arguments @("--git-dir", $PullRemote, "symbolic-ref", "HEAD", "refs/heads/main")
  Invoke-Tool -FilePath $GitExe -Arguments @("clone", "-q", $PullRemote, (Join-Path $WorkDir "git-pull-base"))
  Invoke-Tool -FilePath $ZminGitExe -Arguments @("clone", "-q", $PullRemote, (Join-Path $WorkDir "zmin-pull-base"))
  Configure-Repo -Path (Join-Path $WorkDir "git-pull-base")
  Configure-Repo -Path (Join-Path $WorkDir "zmin-pull-base")

  if (Test-BenchmarkOp "pull-noop") {
    Invoke-Both `
      -Op "pull-noop" `
      -GitArgs @("-C", (Join-Path $WorkDir "git-pull-base"), "pull", "--ff-only") `
      -ZminArgs @("-C", (Join-Path $WorkDir "zmin-pull-base"), "pull", "--ff-only") `
      -Extra "remote"
    Assert-SameRef -Name "pull-noop" -LeftRepo (Join-Path $WorkDir "git-pull-base") -RightRepo (Join-Path $WorkDir "zmin-pull-base") -Ref "HEAD"
  }

  if (Test-BenchmarkOp "pull-incremental") {
    "pull incremental" | Set-Content -LiteralPath (Join-Path $PullSource "pull-incremental.txt") -Encoding UTF8
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", $PullSource, "add", "-A")
    $env:GIT_AUTHOR_DATE = "1700105000 +0000"
    $env:GIT_COMMITTER_DATE = "1700105000 +0000"
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", $PullSource, "commit", "-qm", "pull incremental")
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", $PullSource, "push", "-q", "origin", "main")
    Remove-Item Env:\GIT_AUTHOR_DATE -ErrorAction SilentlyContinue
    Remove-Item Env:\GIT_COMMITTER_DATE -ErrorAction SilentlyContinue

    for ($n = 1; $n -le $Repeats; $n++) {
      $gitPull = Join-Path $WorkDir "git-pull-inc-$n"
      $zminPull = Join-Path $WorkDir "zmin-pull-inc-$n"
      Copy-Item -Recurse -LiteralPath (Join-Path $WorkDir "git-pull-base") -Destination $gitPull
      Copy-Item -Recurse -LiteralPath (Join-Path $WorkDir "zmin-pull-base") -Destination $zminPull
      Measure-Tool -Tool "git" -Op "pull-incremental" -FilePath $GitExe -Arguments @("-C", $gitPull, "pull", "--ff-only") -Extra "$n/remote"
      Measure-Tool -Tool "zmin" -Op "pull-incremental" -FilePath $ZminGitExe -Arguments @("-C", $zminPull, "pull", "--ff-only") -Extra "$n/remote"
      Assert-SameRef -Name "pull-incremental-$n" -LeftRepo $gitPull -RightRepo $zminPull -Ref "HEAD"
      Assert-SameRef -Name "pull-incremental-source-$n" -LeftRepo $PullSource -RightRepo $zminPull -Ref "HEAD"
    }
  }
}

if (Test-AnyBenchmarkOp @("fetch-noop", "fetch-incremental")) {
  $Remote = Join-Path $WorkDir "remote.git"
  Invoke-Tool -FilePath $GitExe -Arguments @("init", "-q", "--bare", $Remote)
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "remote", "add", "origin", $Remote)
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "push", "-q", "origin", "main")
  Invoke-Tool -FilePath $GitExe -Arguments @("--git-dir", $Remote, "symbolic-ref", "HEAD", "refs/heads/main")
  Invoke-Tool -FilePath $GitExe -Arguments @("clone", "-q", $Remote, (Join-Path $WorkDir "git-fetch"))
  Invoke-Tool -FilePath $ZminGitExe -Arguments @("clone", "-q", $Remote, (Join-Path $WorkDir "zmin-fetch"))
  if ($GixExe) {
    Invoke-Tool -FilePath $GitExe -Arguments @("clone", "-q", $Remote, (Join-Path $WorkDir "gix-fetch"))
    Configure-Repo -Path (Join-Path $WorkDir "gix-fetch")
  }

  if (Test-BenchmarkOp "fetch-noop") {
    Invoke-Both `
      -Op "fetch-noop" `
      -GitArgs @("-C", (Join-Path $WorkDir "git-fetch"), "fetch", "origin") `
      -ZminArgs @("-C", (Join-Path $WorkDir "zmin-fetch"), "fetch", "origin") `
      -Extra "remote"
    Invoke-GixRepeated -Op "fetch-noop" -Arguments @("-r", (Join-Path $WorkDir "gix-fetch"), "fetch", "-r", "origin") -Extra "remote"
    Assert-SameRef -Name "fetch-noop" -LeftRepo (Join-Path $WorkDir "git-fetch") -RightRepo (Join-Path $WorkDir "zmin-fetch") -Ref "refs/remotes/origin/main"
  }

  if (Test-BenchmarkOp "fetch-incremental") {
    "incremental" | Set-Content -LiteralPath (Join-Path $Src "incremental.txt") -Encoding UTF8
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "add", "-A")
    $env:GIT_AUTHOR_DATE = "1700100000 +0000"
    $env:GIT_COMMITTER_DATE = "1700100000 +0000"
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "commit", "-qm", "incremental")
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "push", "-q", "origin", "main")
    Remove-Item Env:\GIT_AUTHOR_DATE -ErrorAction SilentlyContinue
    Remove-Item Env:\GIT_COMMITTER_DATE -ErrorAction SilentlyContinue

    for ($n = 1; $n -le $Repeats; $n++) {
      $gitFetch = Join-Path $WorkDir "git-fetch-inc-$n"
      $zminFetch = Join-Path $WorkDir "zmin-fetch-inc-$n"
      Copy-Item -Recurse -LiteralPath (Join-Path $WorkDir "git-fetch") -Destination $gitFetch
      Copy-Item -Recurse -LiteralPath (Join-Path $WorkDir "zmin-fetch") -Destination $zminFetch
      if ($GixExe) {
        $gixFetch = Join-Path $WorkDir "gix-fetch-inc-$n"
        Copy-Item -Recurse -LiteralPath (Join-Path $WorkDir "gix-fetch") -Destination $gixFetch
      }
      Measure-Tool -Tool "git" -Op "fetch-incremental" -FilePath $GitExe -Arguments @("-C", $gitFetch, "fetch", "origin") -Extra "$n/remote"
      if ($GixExe) {
        Measure-Tool -Tool "gix" -Op "fetch-incremental" -FilePath $GixExe -Arguments @("-r", $gixFetch, "fetch", "-r", "origin") -Extra "$n/remote"
      }
      Measure-Tool -Tool "zmin" -Op "fetch-incremental" -FilePath $ZminGitExe -Arguments @("-C", $zminFetch, "fetch", "origin") -Extra "$n/remote"
      Assert-SameRef -Name "fetch-incremental-$n" -LeftRepo $gitFetch -RightRepo $zminFetch -Ref "refs/remotes/origin/main"
    }
  }
}

if ($Rows.Count -eq 0) {
  throw "no benchmark operations were selected"
}

$RowsPath = Join-Path $OutDir "bench.csv"
$ChecksPath = Join-Path $OutDir "checks.csv"
$SummaryPath = Join-Path $OutDir "summary.csv"
$ComparisonPath = Join-Path $OutDir "comparison.csv"
$Rows | Export-Csv -NoTypeInformation -Path $RowsPath
$Checks | Export-Csv -NoTypeInformation -Path $ChecksPath

$Summary = $Rows |
  Group-Object op, tool |
  ForEach-Object {
    $parts = $_.Name -split ", "
    $values = @($_.Group | ForEach-Object { [double]$_.seconds } | Sort-Object)
    [pscustomobject]@{
      op = $parts[0]
      tool = $parts[1]
      runs = $values.Count
      mean_seconds = [Math]::Round(($values | Measure-Object -Average).Average, 6)
      median_seconds = [Math]::Round((Get-Median -Values $values), 6)
      min_seconds = [Math]::Round($values[0], 6)
      max_seconds = [Math]::Round($values[$values.Count - 1], 6)
    }
  } |
  Sort-Object op, tool

$Summary | Export-Csv -NoTypeInformation -Path $SummaryPath

$Comparison = $Rows |
  Group-Object op |
  ForEach-Object {
    $op = $_.Name
    $gitValues = @(
      $_.Group |
        Where-Object { $_.tool -eq "git" } |
        ForEach-Object { [double]$_.seconds } |
        Sort-Object
    )
    $zminValues = @(
      $_.Group |
        Where-Object { $_.tool -eq "zmin" } |
        ForEach-Object { [double]$_.seconds } |
        Sort-Object
    )
    $gixValues = @(
      $_.Group |
        Where-Object { $_.tool -eq "gix" } |
        ForEach-Object { [double]$_.seconds } |
        Sort-Object
    )
    if ($gitValues.Count -eq 0 -or $zminValues.Count -eq 0) {
      return
    }

    $gitMean = [Math]::Round(($gitValues | Measure-Object -Average).Average, 6)
    $zminMean = [Math]::Round(($zminValues | Measure-Object -Average).Average, 6)
    $gitMedian = [Math]::Round((Get-Median -Values $gitValues), 6)
    $zminMedian = [Math]::Round((Get-Median -Values $zminValues), 6)
    $gixMean = $null
    $gixMedian = $null
    $zminVsGixMeanRatio = $null
    $zminVsGixMedianRatio = $null
    if ($gixValues.Count -gt 0) {
      $gixMean = [Math]::Round(($gixValues | Measure-Object -Average).Average, 6)
      $gixMedian = [Math]::Round((Get-Median -Values $gixValues), 6)
      $zminVsGixMeanRatio = Get-Ratio -Numerator $zminMean -Denominator $gixMean
      $zminVsGixMedianRatio = Get-Ratio -Numerator $zminMedian -Denominator $gixMedian
    }
    $zminVsGitPairs = @(Get-PairedRatios -Rows $_.Group -Op $op -NumeratorTool "zmin" -DenominatorTool "git")
    $zminVsGixPairs = @(Get-PairedRatios -Rows $_.Group -Op $op -NumeratorTool "zmin" -DenominatorTool "gix")
    [pscustomobject]@{
      op = $op
      runs = [Math]::Min($gitValues.Count, $zminValues.Count)
      git_mean_seconds = $gitMean
      zmin_mean_seconds = $zminMean
      zmin_vs_git_mean_ratio = Get-Ratio -Numerator $zminMean -Denominator $gitMean
      gix_mean_seconds = $gixMean
      zmin_vs_gix_mean_ratio = $zminVsGixMeanRatio
      git_median_seconds = $gitMedian
      zmin_median_seconds = $zminMedian
      zmin_vs_git_median_ratio = Get-Ratio -Numerator $zminMedian -Denominator $gitMedian
      gix_median_seconds = $gixMedian
      zmin_vs_gix_median_ratio = $zminVsGixMedianRatio
      zmin_vs_git_pair_count = $zminVsGitPairs.Count
      zmin_vs_git_pair_mean_ratio = if ($zminVsGitPairs.Count -eq 0) { $null } else { [Math]::Round(($zminVsGitPairs | Measure-Object -Average).Average, 6) }
      zmin_vs_git_pair_median_ratio = if ($zminVsGitPairs.Count -eq 0) { $null } else { [Math]::Round((Get-Median -Values $zminVsGitPairs), 6) }
      zmin_vs_git_pair_min_ratio = if ($zminVsGitPairs.Count -eq 0) { $null } else { [Math]::Round($zminVsGitPairs[0], 6) }
      zmin_vs_git_pair_max_ratio = if ($zminVsGitPairs.Count -eq 0) { $null } else { [Math]::Round($zminVsGitPairs[$zminVsGitPairs.Count - 1], 6) }
      zmin_vs_gix_pair_count = if ($zminVsGixPairs.Count -eq 0) { $null } else { $zminVsGixPairs.Count }
      zmin_vs_gix_pair_mean_ratio = if ($zminVsGixPairs.Count -eq 0) { $null } else { [Math]::Round(($zminVsGixPairs | Measure-Object -Average).Average, 6) }
      zmin_vs_gix_pair_median_ratio = if ($zminVsGixPairs.Count -eq 0) { $null } else { [Math]::Round((Get-Median -Values $zminVsGixPairs), 6) }
    }
  } |
  Sort-Object op

$Comparison | Export-Csv -NoTypeInformation -Path $ComparisonPath

Write-Host "Windows native benchmark complete"
Write-Host "rows=$RowsPath"
Write-Host "checks=$ChecksPath"
Write-Host "summary=$SummaryPath"
Write-Host "comparison=$ComparisonPath"
if ($ZminPhaseTraceDir) {
  Write-Host "phase_traces=$ZminPhaseTraceDir"
}
$Summary | Format-Table -AutoSize
$Comparison | Format-Table -AutoSize
