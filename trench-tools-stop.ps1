$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path

function Show-Usage {
  Write-Host "Usage: .\trench-tools-stop.ps1 [--mode ee|ld|both]"
}

function Load-DotEnv {
  $envPath = Join-Path $projectRoot ".env"
  if (-not (Test-Path $envPath)) {
    return
  }
  foreach ($line in [System.IO.File]::ReadAllLines($envPath)) {
    $trimmed = ($line -replace "`r$", "").Trim()
    if (-not $trimmed -or $trimmed.StartsWith("#")) {
      continue
    }
    if ($trimmed -match "^export\s+(.+)$") {
      $trimmed = $Matches[1]
    }
    $separatorIndex = $trimmed.IndexOf("=")
    if ($separatorIndex -lt 1) {
      continue
    }
    $name = $trimmed.Substring(0, $separatorIndex).Trim()
    $value = $trimmed.Substring($separatorIndex + 1)
    if ($name -notmatch '^[A-Za-z_][A-Za-z0-9_]*$') {
      continue
    }
    if ($value.Length -ge 2) {
      $first = $value.Substring(0, 1)
      $last = $value.Substring($value.Length - 1, 1)
      if (($first -eq '"' -and $last -eq '"') -or ($first -eq "'" -and $last -eq "'")) {
        $value = $value.Substring(1, $value.Length - 2)
      }
    }
    [Environment]::SetEnvironmentVariable($name, $value, "Process")
  }
}

function Resolve-PathFromProject {
  param(
    [Parameter(Mandatory = $true)]
    [string]$RawPath
  )

  if ([System.IO.Path]::IsPathRooted($RawPath)) {
    return $RawPath
  }
  return Join-Path $projectRoot $RawPath
}

function Parse-ModeArgument {
  param(
    [string[]]$Arguments
  )

  $cliMode = ""
  for ($index = 0; $index -lt $Arguments.Count; $index++) {
    $argument = $Arguments[$index]
    switch -Regex ($argument) {
      '^(--mode|-mode|-Mode)$' {
        $index++
        if ($index -ge $Arguments.Count) {
          throw "--mode requires a value."
        }
        $cliMode = $Arguments[$index]
      }
      '^(--help|-h|-H|/\?)$' {
        Show-Usage
        exit 0
      }
      default {
        throw "Unknown argument: $argument"
      }
    }
  }
  return $cliMode
}

function Get-ValidatedMode {
  param(
    [string]$RequestedMode
  )

  # Default to `both` when no mode is supplied, so a plain `trench-tools-stop`
  # always cleans up every process this workspace could have started. Honouring
  # TRENCH_TOOLS_MODE here would silently skip binaries that a previous run
  # started under a different one-off mode.
  $candidate = if ([string]::IsNullOrWhiteSpace($RequestedMode)) { "both" } else { $RequestedMode }
  $normalized = $candidate.Trim().ToLowerInvariant()
  switch ($normalized) {
    "ee" { return "ee" }
    "ld" { return "ld" }
    "both" { return "both" }
    default { throw "mode must be ee, ld, or both." }
  }
}

function Get-TargetsForMode {
  param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("ee", "ld", "both")]
    [string]$Mode
  )

  switch ($Mode) {
    "ee" { return @("execution-engine") }
    "ld" { return @("launchdeck-engine", "launchdeck-follow-daemon") }
    default { return @("execution-engine", "launchdeck-engine", "launchdeck-follow-daemon") }
  }
}

function Get-PortForBinary {
  param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryName
  )

  switch ($BinaryName) {
    "execution-engine" { return [int]$env:EXECUTION_ENGINE_PORT }
    "launchdeck-engine" { return [int]$env:LAUNCHDECK_PORT }
    "launchdeck-follow-daemon" { return [int]$env:LAUNCHDECK_FOLLOW_DAEMON_PORT }
    default { return 0 }
  }
}

function Get-ListeningPidsForPort {
  param(
    [Parameter(Mandatory = $true)]
    [int]$Port
  )

  if ($Port -le 0) {
    return @()
  }

  $pids = New-Object System.Collections.Generic.List[int]
  $netstatOutput = netstat -ano -p tcp | Select-String -Pattern "127\.0\.0\.1:$Port\s+.*LISTENING\s+(\d+)$"
  foreach ($line in $netstatOutput) {
    if ($line.Matches.Count -eq 0) {
      continue
    }
    $matchedPid = [int]$line.Matches[0].Groups[1].Value
    if (-not $pids.Contains($matchedPid)) {
      $pids.Add($matchedPid)
    }
  }
  return $pids
}

function Test-ProcessMatchesBinary {
  param(
    [Parameter(Mandatory = $true)]
    [System.Diagnostics.Process]$Process,
    [string]$ExpectedBinary
  )

  if ([string]::IsNullOrWhiteSpace($ExpectedBinary)) {
    return $true
  }

  $names = @()
  if ($Process.ProcessName) { $names += $Process.ProcessName }
  try {
    $mainModule = $Process.MainModule
    if ($mainModule -and $mainModule.FileName) {
      $names += [System.IO.Path]::GetFileNameWithoutExtension($mainModule.FileName)
      $names += [System.IO.Path]::GetFileName($mainModule.FileName)
    }
  } catch {
  }

  foreach ($name in $names) {
    if ([string]::IsNullOrWhiteSpace($name)) { continue }
    if ($name -ieq $ExpectedBinary) { return $true }
    if ($name -ieq "$ExpectedBinary.exe") { return $true }
  }
  return $false
}

function Get-ProcessCommandLine {
  param(
    [Parameter(Mandatory = $true)]
    [int]$ProcessId
  )

  try {
    $processRecord = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $ProcessId" -ErrorAction Stop
    return [string]$processRecord.CommandLine
  } catch {
    return ""
  }
}

function Test-ProcessMatchesCommandLineFragment {
  param(
    [Parameter(Mandatory = $true)]
    [System.Diagnostics.Process]$Process,
    [string]$ExpectedFragment
  )

  if ([string]::IsNullOrWhiteSpace($ExpectedFragment)) {
    return $true
  }

  $commandLine = Get-ProcessCommandLine -ProcessId $Process.Id
  if ([string]::IsNullOrWhiteSpace($commandLine)) {
    return $false
  }

  return $commandLine.IndexOf($ExpectedFragment, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
}

function Get-LogViewerMarker {
  param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryName
  )

  return "trench-tools-log-viewer:$BinaryName"
}

function Resolve-TrackedProcess {
  param(
    [Parameter(Mandatory = $true)]
    [int]$ProcessId,
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [string]$ExpectedBinary = "",
    [string]$ExpectedCommandLineFragment = ""
  )

  if ($ProcessId -le 0 -or $ProcessId -eq $PID) {
    return $null
  }

  $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
  if (-not $process) {
    return $null
  }

  if (-not (Test-ProcessMatchesBinary -Process $process -ExpectedBinary $ExpectedBinary)) {
    Write-Warning "Skipping PID $ProcessId for ${Label}: process name '$($process.ProcessName)' does not match expected binary '$ExpectedBinary' (likely a recycled PID in a stale file)."
    return $null
  }
  if (-not (Test-ProcessMatchesCommandLineFragment -Process $process -ExpectedFragment $ExpectedCommandLineFragment)) {
    Write-Warning "Skipping PID $ProcessId for ${Label}: process command line no longer matches the tracked launcher metadata (likely a recycled PID in a stale file)."
    return $null
  }

  return $process
}

function Add-StopRequest {
  param(
    [Parameter(Mandatory = $true)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$Requests,
    [Parameter(Mandatory = $true)]
    [int]$ProcessId,
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [string]$ExpectedBinary = "",
    [string]$ExpectedCommandLineFragment = ""
  )

  foreach ($existing in $Requests) {
    if ($existing.ProcessId -eq $ProcessId) {
      return
    }
  }

  $Requests.Add([pscustomobject]@{
    ProcessId = $ProcessId
    Label = $Label
    ExpectedBinary = $ExpectedBinary
    ExpectedCommandLineFragment = $ExpectedCommandLineFragment
  }) | Out-Null
}

function Request-TrackedProcessStop {
  param(
    [Parameter(Mandatory = $true)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$Requests,
    [Parameter(Mandatory = $true)]
    [int]$ProcessId,
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [string]$ExpectedBinary = "",
    [string]$ExpectedCommandLineFragment = ""
  )

  $process = Resolve-TrackedProcess `
    -ProcessId $ProcessId `
    -Label $Label `
    -ExpectedBinary $ExpectedBinary `
    -ExpectedCommandLineFragment $ExpectedCommandLineFragment
  if (-not $process) {
    return
  }

  try {
    Stop-Process -Id $ProcessId -ErrorAction Stop
  } catch {
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $process) {
      Write-Host "Stopped $Label (PID $ProcessId)."
      return
    }
  }

  Add-StopRequest `
    -Requests $Requests `
    -ProcessId $ProcessId `
    -Label $Label `
    -ExpectedBinary $ExpectedBinary `
    -ExpectedCommandLineFragment $ExpectedCommandLineFragment
}

function Wait-ForStopRequests {
  param(
    [Parameter(Mandatory = $true)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$Requests
  )

  if ($Requests.Count -eq 0) {
    return
  }

  $pending = New-Object System.Collections.Generic.List[object]
  foreach ($request in $Requests) {
    $pending.Add($request) | Out-Null
  }

  for ($attempt = 0; $attempt -lt 20; $attempt++) {
    for ($index = $pending.Count - 1; $index -ge 0; $index--) {
      $request = $pending[$index]
      $process = Get-Process -Id $request.ProcessId -ErrorAction SilentlyContinue
      if (-not $process) {
        Write-Host "Stopped $($request.Label) (PID $($request.ProcessId))."
        $pending.RemoveAt($index)
      }
    }
    if ($pending.Count -eq 0) {
      return
    }
    Start-Sleep -Milliseconds 500
  }

  for ($index = $pending.Count - 1; $index -ge 0; $index--) {
    $request = $pending[$index]
    $process = Resolve-TrackedProcess `
      -ProcessId $request.ProcessId `
      -Label $request.Label `
      -ExpectedBinary $request.ExpectedBinary `
      -ExpectedCommandLineFragment $request.ExpectedCommandLineFragment
    if (-not $process) {
      $pending.RemoveAt($index)
      continue
    }
    Stop-Process -Id $request.ProcessId -Force -ErrorAction SilentlyContinue
  }

  for ($attempt = 0; $attempt -lt 10; $attempt++) {
    for ($index = $pending.Count - 1; $index -ge 0; $index--) {
      $request = $pending[$index]
      $process = Get-Process -Id $request.ProcessId -ErrorAction SilentlyContinue
      if (-not $process) {
        Write-Host "Stopped $($request.Label) (PID $($request.ProcessId)) after force kill."
        $pending.RemoveAt($index)
      }
    }
    if ($pending.Count -eq 0) {
      return
    }
    Start-Sleep -Milliseconds 200
  }

  foreach ($request in $pending) {
    Write-Warning "Failed to stop $($request.Label) (PID $($request.ProcessId))."
  }
}

function Queue-LogViewerStop {
  param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryName,
    [Parameter(Mandatory = $true)]
    [string]$RunDirectory,
    [Parameter(Mandatory = $true)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$Requests
  )

  $viewerPidFile = Join-Path $RunDirectory "$BinaryName.log-viewer.pid"
  if (-not (Test-Path $viewerPidFile)) {
    return
  }

  $rawPid = [System.IO.File]::ReadAllText($viewerPidFile).Trim()
  if ($rawPid) {
    Request-TrackedProcessStop `
      -Requests $Requests `
      -ProcessId ([int]$rawPid) `
      -Label "$BinaryName log viewer" `
      -ExpectedCommandLineFragment (Get-LogViewerMarker -BinaryName $BinaryName)
  }
  Remove-Item $viewerPidFile -Force -ErrorAction SilentlyContinue
}

function Queue-BinaryStops {
  param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryName,
    [Parameter(Mandatory = $true)]
    [string]$RunDirectory,
    [Parameter(Mandatory = $true)]
    [AllowEmptyCollection()]
    [System.Collections.Generic.List[object]]$Requests
  )

  Queue-LogViewerStop -BinaryName $BinaryName -RunDirectory $RunDirectory -Requests $Requests

  $pidFile = Join-Path $RunDirectory "$BinaryName.pid"
  if (Test-Path $pidFile) {
    $rawPid = [System.IO.File]::ReadAllText($pidFile).Trim()
    if ($rawPid) {
      Request-TrackedProcessStop `
        -Requests $Requests `
        -ProcessId ([int]$rawPid) `
        -Label $BinaryName `
        -ExpectedBinary $BinaryName
    }
    Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
  }

  $port = Get-PortForBinary -BinaryName $BinaryName
  foreach ($listeningPid in Get-ListeningPidsForPort -Port $port) {
    Request-TrackedProcessStop `
      -Requests $Requests `
      -ProcessId $listeningPid `
      -Label "$BinaryName listener on :$port" `
      -ExpectedBinary $BinaryName
  }
}

$cliMode = Parse-ModeArgument -Arguments $args
Load-DotEnv
$mode = Get-ValidatedMode -RequestedMode $cliMode

if ([string]::IsNullOrWhiteSpace($env:EXECUTION_ENGINE_PORT)) { $env:EXECUTION_ENGINE_PORT = "8788" }
if ([string]::IsNullOrWhiteSpace($env:LAUNCHDECK_PORT)) { $env:LAUNCHDECK_PORT = "8789" }
if ([string]::IsNullOrWhiteSpace($env:LAUNCHDECK_FOLLOW_DAEMON_PORT)) { $env:LAUNCHDECK_FOLLOW_DAEMON_PORT = "8790" }
if ([string]::IsNullOrWhiteSpace($env:TRENCH_TOOLS_DATA_ROOT)) { $env:TRENCH_TOOLS_DATA_ROOT = ".local/trench-tools" }

$dataRoot = Resolve-PathFromProject -RawPath $env:TRENCH_TOOLS_DATA_ROOT
$runDir = Join-Path $dataRoot "run"
New-Item -ItemType Directory -Path $runDir -Force | Out-Null

$stopRequests = New-Object System.Collections.Generic.List[object]
Write-Host "Stopping trench tools ($mode)..."
foreach ($binary in Get-TargetsForMode -Mode $mode) {
  Queue-BinaryStops -BinaryName $binary -RunDirectory $runDir -Requests $stopRequests
}
Wait-ForStopRequests -Requests $stopRequests
