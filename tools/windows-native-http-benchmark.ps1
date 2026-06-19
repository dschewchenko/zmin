param(
  [int]$Repeats = 3,
  [int]$Commits = 40,
  [int]$FilesPerCommit = 20,
  [int]$BatchFiles = 800,
  [string]$OutDir = "",
  [double]$MaxZminVsGitMeanRatio = 0.0,
  [double]$MaxZminVsGitMedianRatio = 0.0
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

if (-not $OutDir) {
  $OutDir = Join-Path ([System.IO.Path]::GetTempPath()) ("zmin-windows-http-bench-" + [Guid]::NewGuid().ToString("N"))
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$WorkDir = Join-Path $OutDir "work"
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
$Rows = New-Object System.Collections.Generic.List[object]
$Checks = New-Object System.Collections.Generic.List[object]

function Invoke-Tool {
  param(
    [Parameter(Mandatory = $true)]
    [string]$FilePath,

    [string[]]$Arguments = @(),

    [string]$WorkingDirectory = (Get-Location).Path
  )

  $errPath = Join-Path $WorkDir ("stderr-" + [Guid]::NewGuid().ToString("N") + ".txt")
  Push-Location $WorkingDirectory
  $previousErrorActionPreference = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try {
    & $FilePath @Arguments > $null 2> $errPath
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
  Invoke-Tool -FilePath $FilePath -Arguments $Arguments -WorkingDirectory $WorkingDirectory
  $sw.Stop()

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

  $actual = & $GitExe -C $Repo config --get $Key
  if ($LASTEXITCODE -ne 0 -or $actual -ne $Expected) {
    Add-Check -Name $Name -Status "fail" -Details "${Key}: ${actual} != ${Expected}"
    throw "$Name config mismatch"
  }
  Add-Check -Name $Name -Status "ok" -Details "${Key}=${actual}"
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

function Assert-ComparisonMaxRatio {
  param(
    [Parameter(Mandatory = $true)]
    [object[]]$ComparisonRows,

    [Parameter(Mandatory = $true)]
    [string]$Column,

    [Parameter(Mandatory = $true)]
    [double]$MaxRatio,

    [Parameter(Mandatory = $true)]
    [string]$Label
  )

  if ($MaxRatio -le 0.0) {
    return
  }

  $failures = New-Object System.Collections.Generic.List[string]
  foreach ($row in $ComparisonRows) {
    $property = $row.PSObject.Properties[$Column]
    if ($null -eq $property -or $null -eq $property.Value -or "$($property.Value)" -eq "") {
      $failures.Add("$($row.op): missing $Label")
      continue
    }

    $ratio = [double]$property.Value
    if ($ratio -gt $MaxRatio) {
      $failures.Add("$($row.op): $Label $ratio > $MaxRatio")
    }
  }

  if ($failures.Count -gt 0) {
    throw "benchmark ratio gate failed for ${Label}: $($failures -join '; ')"
  }
}

function Configure-Repo {
  param([string]$Path)

  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Path, "config", "user.name", "Bench")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Path, "config", "user.email", "bench@example.test")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Path, "config", "commit.gpgsign", "false")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Path, "config", "core.autocrlf", "false")
}

function Write-FixtureFile {
  param(
    [string]$Path,
    [string]$Label,
    [int]$Index
  )

  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
  "label=$Label index=$Index payload=$('0' * 4096)" | Set-Content -LiteralPath $Path -Encoding UTF8
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

$ServerScript = {
  param(
    [int]$Port,
    [string]$GitExe,
    [string]$ProjectRoot,
    [string]$StopFile,
    [string]$LogPath
  )

  $ErrorActionPreference = "Stop"

  function Find-HeaderEnd {
    param([byte[]]$Bytes)

    for ($i = 0; $i -le $Bytes.Length - 4; $i++) {
      if ($Bytes[$i] -eq 13 -and $Bytes[$i + 1] -eq 10 -and $Bytes[$i + 2] -eq 13 -and $Bytes[$i + 3] -eq 10) {
        return $i
      }
    }
    return -1
  }

  function Invoke-Backend {
    param(
      [string]$Method,
      [string]$Path,
      [string]$Query,
      [hashtable]$Headers,
      [byte[]]$Body
    )

    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $GitExe
    $psi.Arguments = "http-backend"
    $psi.UseShellExecute = $false
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.Environment["GIT_PROJECT_ROOT"] = $ProjectRoot
    $psi.Environment["GIT_HTTP_EXPORT_ALL"] = "1"
    $psi.Environment["REQUEST_METHOD"] = $Method
    $psi.Environment["PATH_INFO"] = $Path
    $psi.Environment["QUERY_STRING"] = $Query
    $psi.Environment["SERVER_PROTOCOL"] = "HTTP/1.1"
    $psi.Environment["GATEWAY_INTERFACE"] = "CGI/1.1"
    $psi.Environment["REMOTE_ADDR"] = "127.0.0.1"
    $psi.Environment["CONTENT_LENGTH"] = $Body.Length.ToString()
    if ($Headers.ContainsKey("content-type")) {
      $psi.Environment["CONTENT_TYPE"] = [string]$Headers["content-type"]
    }

    $process = [System.Diagnostics.Process]::Start($psi)
    if ($Body.Length -gt 0) {
      $process.StandardInput.BaseStream.Write($Body, 0, $Body.Length)
    }
    $process.StandardInput.Close()
    $stdout = [System.IO.MemoryStream]::new()
    $process.StandardOutput.BaseStream.CopyTo($stdout)
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) {
      throw "git http-backend failed with exit code $($process.ExitCode): $stderr"
    }
    return $stdout.ToArray()
  }

  function Write-HttpResponse {
    param(
      [System.Net.Sockets.NetworkStream]$Stream,
      [byte[]]$Backend
    )

    $headerEnd = Find-HeaderEnd -Bytes $Backend
    if ($headerEnd -lt 0) {
      $response = [System.Text.Encoding]::ASCII.GetBytes("HTTP/1.1 500 Internal Server Error`r`nContent-Length: 0`r`nConnection: close`r`n`r`n")
      $Stream.Write($response, 0, $response.Length)
      return
    }

    $headersText = [System.Text.Encoding]::ASCII.GetString($Backend, 0, $headerEnd)
    $bodyOffset = $headerEnd + 4
    $bodyLength = $Backend.Length - $bodyOffset
    $body = [byte[]]::new($bodyLength)
    if ($bodyLength -gt 0) {
      [Array]::Copy($Backend, $bodyOffset, $body, 0, $bodyLength)
    }

    $status = "200 OK"
    $headers = New-Object System.Collections.Generic.List[string]
    foreach ($line in ($headersText -split "`r?`n")) {
      if (-not $line) {
        continue
      }
      if ($line.StartsWith("Status:", [System.StringComparison]::OrdinalIgnoreCase)) {
        $status = $line.Substring(7).Trim()
      } elseif (-not $line.StartsWith("Content-Length:", [System.StringComparison]::OrdinalIgnoreCase)) {
        $headers.Add($line) | Out-Null
      }
    }
    $headers.Add("Content-Length: $bodyLength") | Out-Null
    $headers.Add("Connection: close") | Out-Null
    $prefix = "HTTP/1.1 $status`r`n$($headers -join "`r`n")`r`n`r`n"
    $prefixBytes = [System.Text.Encoding]::ASCII.GetBytes($prefix)
    $Stream.Write($prefixBytes, 0, $prefixBytes.Length)
    if ($bodyLength -gt 0) {
      $Stream.Write($body, 0, $body.Length)
    }
  }

  function Write-EmptyHttpResponse {
    param(
      [System.Net.Sockets.NetworkStream]$Stream,
      [string]$Status
    )

    $response = [System.Text.Encoding]::ASCII.GetBytes("HTTP/1.1 $Status`r`nContent-Length: 0`r`nConnection: close`r`n`r`n")
    $Stream.Write($response, 0, $response.Length)
  }

  function Handle-Client {
    param([System.Net.Sockets.TcpClient]$Client)

    try {
      $stream = $Client.GetStream()
      $buffer = [byte[]]::new(8192)
      $request = [System.IO.MemoryStream]::new()
      $headerEnd = -1
      while ($headerEnd -lt 0) {
        $read = $stream.Read($buffer, 0, $buffer.Length)
        if ($read -le 0) {
          return
        }
        $request.Write($buffer, 0, $read)
        $headerEnd = Find-HeaderEnd -Bytes $request.ToArray()
      }

      $requestBytes = $request.ToArray()
      $headersText = [System.Text.Encoding]::ASCII.GetString($requestBytes, 0, $headerEnd)
      $lines = $headersText -split "`r?`n"
      if ($lines.Length -eq 0 -or [string]::IsNullOrWhiteSpace($lines[0])) {
        Write-EmptyHttpResponse -Stream $stream -Status "400 Bad Request"
        return
      }
      $requestLine = $lines[0].Split(" ")
      if ($requestLine.Length -lt 2) {
        Write-EmptyHttpResponse -Stream $stream -Status "400 Bad Request"
        return
      }
      $method = $requestLine[0]
      $rawPath = $requestLine[1]
      $path = $rawPath
      $query = ""
      $queryIndex = $rawPath.IndexOf("?")
      if ($queryIndex -ge 0) {
        $path = $rawPath.Substring(0, $queryIndex)
        $query = $rawPath.Substring($queryIndex + 1)
      }

      $headers = @{}
      for ($i = 1; $i -lt $lines.Length; $i++) {
        $line = $lines[$i]
        $colon = $line.IndexOf(":")
        if ($colon -gt 0) {
          $headers[$line.Substring(0, $colon).Trim().ToLowerInvariant()] = $line.Substring($colon + 1).Trim()
        }
      }

      $contentLength = 0
      if ($headers.ContainsKey("content-length")) {
        $contentLength = [int]$headers["content-length"]
      }

      $bodyOffset = $headerEnd + 4
      $body = [System.IO.MemoryStream]::new()
      if ($requestBytes.Length -gt $bodyOffset) {
        $body.Write($requestBytes, $bodyOffset, $requestBytes.Length - $bodyOffset)
      }
      while ($body.Length -lt $contentLength) {
        $read = $stream.Read($buffer, 0, [Math]::Min($buffer.Length, $contentLength - [int]$body.Length))
        if ($read -le 0) {
          break
        }
        $body.Write($buffer, 0, $read)
      }

      $backend = Invoke-Backend -Method $method -Path $path -Query $query -Headers $headers -Body $body.ToArray()
      Write-HttpResponse -Stream $stream -Backend $backend
    } catch {
      Add-Content -LiteralPath $LogPath -Value $_.Exception.ToString()
    } finally {
      $Client.Close()
    }
  }

  $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Parse("127.0.0.1"), $Port)
  $listener.Start()
  try {
    while (-not (Test-Path -LiteralPath $StopFile)) {
      if (-not $listener.Pending()) {
        Start-Sleep -Milliseconds 50
        continue
      }
      $client = $listener.AcceptTcpClient()
      Handle-Client -Client $client
    }
  } finally {
    $listener.Stop()
  }
}

function Start-GitHttpBackendServer {
  param(
    [string]$ProjectRoot,
    [string]$LogPath
  )

  $probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Parse("127.0.0.1"), 0)
  $probe.Start()
  $port = ([System.Net.IPEndPoint]$probe.LocalEndpoint).Port
  $probe.Stop()

  $stopFile = Join-Path $WorkDir ("stop-http-" + [Guid]::NewGuid().ToString("N"))
  $job = Start-Job -ScriptBlock $ServerScript -ArgumentList @($port, $GitExe, $ProjectRoot, $stopFile, $LogPath)
  return [pscustomobject]@{
    Port = $port
    Url = "http://127.0.0.1:$port/remote.git"
    StopFile = $stopFile
    Job = $job
  }
}

function Stop-GitHttpBackendServer {
  param($Server)

  if ($null -eq $Server) {
    return
  }
  New-Item -ItemType File -Force -Path $Server.StopFile | Out-Null
  Wait-Job -Job $Server.Job -Timeout 5 | Out-Null
  if ($Server.Job.State -eq "Running") {
    Stop-Job -Job $Server.Job -Force
  }
  Receive-Job -Job $Server.Job | Out-Null
  Remove-Job -Job $Server.Job -Force
}

$Src = Join-Path $WorkDir "src"
$Repos = Join-Path $WorkDir "repos"
Invoke-Tool -FilePath $GitExe -Arguments @("init", "-q", "-b", "main", $Src)
Configure-Repo -Path $Src

for ($c = 1; $c -le $Commits; $c++) {
  for ($f = 1; $f -le $FilesPerCommit; $f++) {
    Write-FixtureFile -Path (Join-Path $Src ("dir-" + ($c % 24) + "\file-$f.txt")) -Label "commit-$c-file" -Index $f
  }
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "add", "-A")
  $env:GIT_AUTHOR_DATE = (1800000000 + $c).ToString() + " +0000"
  $env:GIT_COMMITTER_DATE = $env:GIT_AUTHOR_DATE
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "commit", "-qm", "commit $c")
}
Remove-Item Env:\GIT_AUTHOR_DATE -ErrorAction SilentlyContinue
Remove-Item Env:\GIT_COMMITTER_DATE -ErrorAction SilentlyContinue
Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "repack", "-adq")
Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "fsck", "--strict")
New-Item -ItemType Directory -Force -Path $Repos | Out-Null
Invoke-Tool -FilePath $GitExe -Arguments @("clone", "-q", "--bare", $Src, (Join-Path $Repos "remote.git"))

$ServerLog = Join-Path $OutDir "http-server.log"
$Server = Start-GitHttpBackendServer -ProjectRoot $Repos -LogPath $ServerLog
try {
  for ($i = 0; $i -lt 100; $i++) {
    try {
      Invoke-Tool -FilePath $GitExe -Arguments @("ls-remote", $Server.Url, "HEAD")
      break
    } catch {
      if ($i -eq 99) {
        throw
      }
      Start-Sleep -Milliseconds 100
    }
  }

  for ($n = 1; $n -le $Repeats; $n++) {
    $gitClone = Join-Path $WorkDir "git-http-clone-$n"
    $zminClone = Join-Path $WorkDir "zmin-http-clone-$n"
    if (($n % 2) -eq 0) {
      Measure-Tool -Tool "zmin" -Op "clone-http" -FilePath $ZminGitExe -Arguments @("clone", "-q", $Server.Url, $zminClone) -Extra "$n/smart-http"
      Measure-Tool -Tool "git" -Op "clone-http" -FilePath $GitExe -Arguments @("clone", "-q", $Server.Url, $gitClone) -Extra "$n/smart-http"
    } else {
      Measure-Tool -Tool "git" -Op "clone-http" -FilePath $GitExe -Arguments @("clone", "-q", $Server.Url, $gitClone) -Extra "$n/smart-http"
      Measure-Tool -Tool "zmin" -Op "clone-http" -FilePath $ZminGitExe -Arguments @("clone", "-q", $Server.Url, $zminClone) -Extra "$n/smart-http"
    }
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", $zminClone, "fsck", "--strict")
    Assert-SameRef -Name "clone-http-$n" -LeftRepo $gitClone -RightRepo $zminClone -Ref "HEAD"
  }

  for ($n = 1; $n -le $Repeats; $n++) {
    $gitClone = Join-Path $WorkDir "git-http-instant-baseline-$n"
    $zminClone = Join-Path $WorkDir "zmin-http-instant-$n"
    if (($n % 2) -eq 0) {
      Measure-Tool -Tool "zmin" -Op "clone-http-instant" -FilePath $ZminGitExe -Arguments @("clone", "-q", "--instant", $Server.Url, $zminClone) -Extra "$n/smart-http"
      Measure-Tool -Tool "git" -Op "clone-http-instant" -FilePath $GitExe -Arguments @("clone", "-q", $Server.Url, $gitClone) -Extra "$n/smart-http"
    } else {
      Measure-Tool -Tool "git" -Op "clone-http-instant" -FilePath $GitExe -Arguments @("clone", "-q", $Server.Url, $gitClone) -Extra "$n/smart-http"
      Measure-Tool -Tool "zmin" -Op "clone-http-instant" -FilePath $ZminGitExe -Arguments @("clone", "-q", "--instant", $Server.Url, $zminClone) -Extra "$n/smart-http"
    }
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", $zminClone, "fsck", "--strict")
    Assert-SameRef -Name "clone-http-instant-$n" -LeftRepo $gitClone -RightRepo $zminClone -Ref "HEAD"
    Assert-SameRef -Name "clone-http-instant-$n-tree" -LeftRepo $gitClone -RightRepo $zminClone -Ref "HEAD^{tree}"
    Assert-ConfigValue -Name "clone-http-instant-$n-marker" -Repo $zminClone -Key "zmin.worktreeFirst" -Expected "true"
  }

  $GitFetchBase = Join-Path $WorkDir "git-fetch-base"
  $ZminFetchBase = Join-Path $WorkDir "zmin-fetch-base"
  Invoke-Tool -FilePath $GitExe -Arguments @("clone", "-q", $Server.Url, $GitFetchBase)
  Invoke-Tool -FilePath $ZminGitExe -Arguments @("clone", "-q", $Server.Url, $ZminFetchBase)

  Invoke-Both `
    -Op "fetch-http-noop" `
    -GitArgs @("-C", $GitFetchBase, "fetch", "origin") `
    -ZminArgs @("-C", $ZminFetchBase, "fetch", "origin") `
    -Extra "smart-http"
  Assert-SameRef -Name "fetch-http-noop" -LeftRepo $GitFetchBase -RightRepo $ZminFetchBase -Ref "refs/remotes/origin/main"

  "incremental" | Set-Content -LiteralPath (Join-Path $Src "incremental.txt") -Encoding UTF8
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "add", "incremental.txt")
  $env:GIT_AUTHOR_DATE = "1800100000 +0000"
  $env:GIT_COMMITTER_DATE = "1800100000 +0000"
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "commit", "-qm", "incremental")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "push", "-q", (Join-Path $Repos "remote.git"), "main")
  Remove-Item Env:\GIT_AUTHOR_DATE -ErrorAction SilentlyContinue
  Remove-Item Env:\GIT_COMMITTER_DATE -ErrorAction SilentlyContinue

  for ($n = 1; $n -le $Repeats; $n++) {
    $gitFetch = Join-Path $WorkDir "git-fetch-incremental-$n"
    $zminFetch = Join-Path $WorkDir "zmin-fetch-incremental-$n"
    Copy-Item -Recurse -LiteralPath $GitFetchBase -Destination $gitFetch
    Copy-Item -Recurse -LiteralPath $ZminFetchBase -Destination $zminFetch
    if (($n % 2) -eq 0) {
      Measure-Tool -Tool "zmin" -Op "fetch-http-incremental" -FilePath $ZminGitExe -Arguments @("-C", $zminFetch, "fetch", "origin") -Extra "$n/smart-http"
      Measure-Tool -Tool "git" -Op "fetch-http-incremental" -FilePath $GitExe -Arguments @("-C", $gitFetch, "fetch", "origin") -Extra "$n/smart-http"
    } else {
      Measure-Tool -Tool "git" -Op "fetch-http-incremental" -FilePath $GitExe -Arguments @("-C", $gitFetch, "fetch", "origin") -Extra "$n/smart-http"
      Measure-Tool -Tool "zmin" -Op "fetch-http-incremental" -FilePath $ZminGitExe -Arguments @("-C", $zminFetch, "fetch", "origin") -Extra "$n/smart-http"
    }
    Assert-SameRef -Name "fetch-http-incremental-$n" -LeftRepo $gitFetch -RightRepo $zminFetch -Ref "refs/remotes/origin/main"
  }

  for ($i = 1; $i -le $BatchFiles; $i++) {
    Write-FixtureFile -Path (Join-Path $Src ("batch\file-$i.txt")) -Label "batch" -Index $i
  }
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "add", "-A")
  $env:GIT_AUTHOR_DATE = "1800100001 +0000"
  $env:GIT_COMMITTER_DATE = "1800100001 +0000"
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "commit", "-qm", "batch")
  Invoke-Tool -FilePath $GitExe -Arguments @("-C", $Src, "push", "-q", (Join-Path $Repos "remote.git"), "main")
  Remove-Item Env:\GIT_AUTHOR_DATE -ErrorAction SilentlyContinue
  Remove-Item Env:\GIT_COMMITTER_DATE -ErrorAction SilentlyContinue

  for ($n = 1; $n -le $Repeats; $n++) {
    $gitFetch = Join-Path $WorkDir "git-fetch-batch-$n"
    $zminFetch = Join-Path $WorkDir "zmin-fetch-batch-$n"
    Copy-Item -Recurse -LiteralPath $GitFetchBase -Destination $gitFetch
    Copy-Item -Recurse -LiteralPath $ZminFetchBase -Destination $zminFetch
    if (($n % 2) -eq 0) {
      Measure-Tool -Tool "zmin" -Op "fetch-http-batch" -FilePath $ZminGitExe -Arguments @("-C", $zminFetch, "fetch", "origin") -Extra "$n/$BatchFiles files"
      Measure-Tool -Tool "git" -Op "fetch-http-batch" -FilePath $GitExe -Arguments @("-C", $gitFetch, "fetch", "origin") -Extra "$n/$BatchFiles files"
    } else {
      Measure-Tool -Tool "git" -Op "fetch-http-batch" -FilePath $GitExe -Arguments @("-C", $gitFetch, "fetch", "origin") -Extra "$n/$BatchFiles files"
      Measure-Tool -Tool "zmin" -Op "fetch-http-batch" -FilePath $ZminGitExe -Arguments @("-C", $zminFetch, "fetch", "origin") -Extra "$n/$BatchFiles files"
    }
    Invoke-Tool -FilePath $GitExe -Arguments @("-C", $zminFetch, "fsck", "--strict")
    Assert-SameRef -Name "fetch-http-batch-$n" -LeftRepo $gitFetch -RightRepo $zminFetch -Ref "refs/remotes/origin/main"
  }
} finally {
  Stop-GitHttpBackendServer -Server $Server
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
    if ($gitValues.Count -eq 0 -or $zminValues.Count -eq 0) {
      return
    }

    $gitMean = [Math]::Round(($gitValues | Measure-Object -Average).Average, 6)
    $zminMean = [Math]::Round(($zminValues | Measure-Object -Average).Average, 6)
    $gitMedian = [Math]::Round((Get-Median -Values $gitValues), 6)
    $zminMedian = [Math]::Round((Get-Median -Values $zminValues), 6)
    [pscustomobject]@{
      op = $op
      runs = [Math]::Min($gitValues.Count, $zminValues.Count)
      git_mean_seconds = $gitMean
      zmin_mean_seconds = $zminMean
      zmin_vs_git_mean_ratio = Get-Ratio -Numerator $zminMean -Denominator $gitMean
      git_median_seconds = $gitMedian
      zmin_median_seconds = $zminMedian
      zmin_vs_git_median_ratio = Get-Ratio -Numerator $zminMedian -Denominator $gitMedian
    }
  } |
  Sort-Object op

$Comparison | Export-Csv -NoTypeInformation -Path $ComparisonPath
Assert-ComparisonMaxRatio -ComparisonRows @($Comparison) -Column "zmin_vs_git_mean_ratio" -MaxRatio $MaxZminVsGitMeanRatio -Label "Zmin/Git mean"
Assert-ComparisonMaxRatio -ComparisonRows @($Comparison) -Column "zmin_vs_git_median_ratio" -MaxRatio $MaxZminVsGitMedianRatio -Label "Zmin/Git median"

Write-Host "Windows native smart HTTP benchmark complete"
Write-Host "rows=$RowsPath"
Write-Host "checks=$ChecksPath"
Write-Host "summary=$SummaryPath"
Write-Host "comparison=$ComparisonPath"
Write-Host "server_log=$ServerLog"
$Summary | Format-Table -AutoSize
$Comparison | Format-Table -AutoSize
