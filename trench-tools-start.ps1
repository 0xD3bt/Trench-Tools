$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path

function Show-Usage {
  Write-Host "Usage: .\trench-tools-start.ps1 [--mode ee|ld|both]"
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

  $candidate = if ([string]::IsNullOrWhiteSpace($RequestedMode)) {
    if ([string]::IsNullOrWhiteSpace($env:TRENCH_TOOLS_MODE)) { "both" } else { $env:TRENCH_TOOLS_MODE }
  } else {
    $RequestedMode
  }
  $normalized = $candidate.Trim().ToLowerInvariant()
  switch ($normalized) {
    "ee" { return "ee" }
    "ld" { return "ld" }
    "both" { return "both" }
    default { throw "mode must be ee, ld, or both." }
  }
}

function Get-ValidatedTerminalMode {
  param(
    [string]$RequestedMode
  )

  $candidate = if ([string]::IsNullOrWhiteSpace($RequestedMode)) {
    "none"
  } else {
    $RequestedMode
  }
  $normalized = $candidate.Trim().ToLowerInvariant()
  switch ($normalized) {
    "none" { return "none" }
    "logs" { return "logs" }
    default { throw "TRENCH_TOOLS_TERMINALS must be none or logs." }
  }
}

function Get-TargetSpecsForMode {
  param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("ee", "ld", "both")]
    [string]$Mode
  )

  switch ($Mode) {
    "ee" {
      return @(
        [pscustomobject]@{ Package = "execution-engine"; Binary = "execution-engine" }
      )
    }
    "ld" {
      return @(
        [pscustomobject]@{ Package = "launchdeck-engine"; Binary = "launchdeck-engine" },
        [pscustomobject]@{ Package = "launchdeck-engine"; Binary = "launchdeck-follow-daemon" }
      )
    }
    default {
      return @(
        [pscustomobject]@{ Package = "execution-engine"; Binary = "execution-engine" },
        [pscustomobject]@{ Package = "launchdeck-engine"; Binary = "launchdeck-engine" },
        [pscustomobject]@{ Package = "launchdeck-engine"; Binary = "launchdeck-follow-daemon" }
      )
    }
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

function Get-DisplayNameForBinary {
  param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryName
  )

  switch ($BinaryName) {
    "execution-engine" { return "Execution Engine" }
    "launchdeck-engine" { return "LaunchDeck Engine" }
    "launchdeck-follow-daemon" { return "Follow Daemon" }
    default { return $BinaryName }
  }
}

function Get-DescriptionForBinary {
  param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryName
  )

  switch ($BinaryName) {
    "execution-engine" { return "local trading service used by the browser extension" }
    "launchdeck-engine" { return "LaunchDeck API and control service" }
    "launchdeck-follow-daemon" { return "background follow-trading worker" }
    default { return "local service" }
  }
}

function Get-NowMilliseconds {
  return [int64]([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
}

function Format-ElapsedMilliseconds {
  param(
    [Parameter(Mandatory = $true)]
    [int64]$Milliseconds
  )

  if ($Milliseconds -lt 1000) {
    return "$Milliseconds`ms"
  }
  if ($Milliseconds -lt 60000) {
    return ("{0:N1}s" -f ($Milliseconds / 1000)).Replace(",", "")
  }
  return ("{0}m {1:00}s" -f [math]::Floor($Milliseconds / 60000), [math]::Floor(($Milliseconds % 60000) / 1000))
}

function Write-StepRow {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Status,
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [string]$Detail = ""
  )

  Write-Host ("  {0,-5} {1,-28} {2}" -f $Status, $Label, $Detail).TrimEnd()
}

function Start-Step {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [string]$Status = "WAIT",
    [string]$Detail = ""
  )

  $script:CurrentStepLabel = $Label
  $script:CurrentStepStartedAt = Get-NowMilliseconds
  Write-StepRow -Status $Status -Label $Label -Detail $Detail
}

function Complete-Step {
  $elapsed = (Get-NowMilliseconds) - $script:CurrentStepStartedAt
  Write-StepRow -Status "OK" -Label $script:CurrentStepLabel -Detail (Format-ElapsedMilliseconds -Milliseconds $elapsed)
}

function Get-HealthUrlForBinary {
  param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryName
  )

  switch ($BinaryName) {
    "execution-engine" { return "http://127.0.0.1:$($env:EXECUTION_ENGINE_PORT)/api/extension/auth/bootstrap" }
    "launchdeck-engine" { return "http://127.0.0.1:$($env:LAUNCHDECK_PORT)/health" }
    "launchdeck-follow-daemon" { return "http://127.0.0.1:$($env:LAUNCHDECK_FOLLOW_DAEMON_PORT)/health" }
    default { return "" }
  }
}

function Show-StartupOverview {
  param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("ee", "ld", "both")]
    [string]$Mode,
    [Parameter(Mandatory = $true)]
    [object[]]$Targets,
    [Parameter(Mandatory = $true)]
    [string]$LogDirectory
  )

  $modeDescription = switch ($Mode) {
    "ee" { "Execution Engine only" }
    "ld" { "LaunchDeck only" }
    default { "Execution Engine and LaunchDeck" }
  }

  Write-Host ""
  Write-Host "Trench Tools startup"
  Write-Host "Mode: $Mode ($modeDescription)"
  Write-Host "Logs: $LogDirectory"
  Write-Host ""
  Write-Host "Steps"
}

function Get-LogViewerMarker {
  param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryName
  )

  return "trench-tools-log-viewer:$BinaryName"
}

function Rotate-Log {
  param(
    [Parameter(Mandatory = $true)]
    [string]$LogPath
  )

  if (Test-Path "$LogPath.1") {
    Remove-Item "$LogPath.1" -Force
  }
  if (Test-Path $LogPath) {
    Move-Item $LogPath "$LogPath.1" -Force
  }
}

function Get-BinaryPath {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Binary
  )

  return Join-Path $projectRoot "target\release\$Binary.exe"
}

function Build-Targets {
  param(
    [Parameter(Mandatory = $true)]
    [object[]]$Targets
  )

  if ($Targets.Count -eq 0) {
    return
  }

  $cargoArgs = @("build", "--release")
  $binaryNames = New-Object System.Collections.Generic.List[string]
  foreach ($target in $Targets) {
    $cargoArgs += @("--bin", $target.Binary)
    $binaryNames.Add($target.Binary) | Out-Null
  }

  Start-Step -Label "Build services" -Status "BUILD" -Detail ($binaryNames -join ", ")
  Push-Location $projectRoot
  try {
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) {
      throw "cargo build failed for selected binaries."
    }
  } finally {
    Pop-Location
  }

  foreach ($target in $Targets) {
    $binaryPath = Get-BinaryPath -Binary $target.Binary
    if (-not (Test-Path $binaryPath)) {
      throw "Expected built binary at $binaryPath."
    }
  }
  Complete-Step
}

function Wait-ForHealthEndpoint {
  param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryName,
    [Parameter(Mandatory = $true)]
    [int]$ProcessId,
    [Parameter(Mandatory = $true)]
    [string]$LogPath
  )

  switch ($BinaryName) {
    "execution-engine" { $url = Get-HealthUrlForBinary -BinaryName $BinaryName }
    "launchdeck-engine" { $url = Get-HealthUrlForBinary -BinaryName $BinaryName }
    "launchdeck-follow-daemon" { $url = Get-HealthUrlForBinary -BinaryName $BinaryName }
    default { return }
  }

  for ($attempt = 0; $attempt -lt 120; $attempt++) {
    try {
      $response = Invoke-RestMethod -UseBasicParsing $url -TimeoutSec 2
      $isExecutionHealthy = $BinaryName -eq "execution-engine" -and (
        $response.authRequired -eq $true -or $response.status -eq "ready"
      )
      if ($isExecutionHealthy) {
        return
      }
      $isLaunchdeckHealthy = $BinaryName -ne "execution-engine" -and (
        $response.ok -eq $true -or $response.running -eq $true
      )
      if ($isLaunchdeckHealthy) {
        return
      }
    } catch {
    }

    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $process) {
      throw "$BinaryName exited before it became healthy. Check $LogPath."
    }
    Start-Sleep -Milliseconds 500
  }

  throw "$BinaryName did not become healthy. Check $LogPath."
}

function Get-ExecutionEngineAuthTokenPath {
  try {
    $bootstrap = Invoke-RestMethod `
      -UseBasicParsing `
      -Uri "http://127.0.0.1:$($env:EXECUTION_ENGINE_PORT)/api/extension/auth/bootstrap" `
      -TimeoutSec 2
    $tokenFilePath = [string]$bootstrap.tokenFilePath
    if (-not [string]::IsNullOrWhiteSpace($tokenFilePath)) {
      return $tokenFilePath
    }
  } catch {
  }
  return ""
}

function Show-BrowserTunnelGuidance {
  param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("ee", "ld", "both")]
    [string]$Mode
  )

  switch ($Mode) {
    "ee" {
      $forwardedPorts = "8788"
      $manualTunnel = "ssh -L 8788:127.0.0.1:8788 root@YOUR_SERVER_IP"
      $checkCommands = "Test-NetConnection 127.0.0.1 -Port 8788"
    }
    "ld" {
      $forwardedPorts = "8789"
      $manualTunnel = "ssh -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP"
      $checkCommands = "Test-NetConnection 127.0.0.1 -Port 8789"
    }
    default {
      $forwardedPorts = "8788 and 8789"
      $manualTunnel = "ssh -L 8788:127.0.0.1:8788 -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP"
      $checkCommands = "Test-NetConnection 127.0.0.1 -Port 8788; Test-NetConnection 127.0.0.1 -Port 8789"
    }
  }

  Write-Host ""
  Write-Host "Browser tunnel"
  Write-Host "  Remote browser: forward local port(s) $forwardedPorts to this VPS."
  Write-Host "  One-off: $manualTunnel"
  Write-Host "  Do not expose ports 8788, 8789, or 8790 publicly."
}

function Show-FinalSummary {
  param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("ee", "ld", "both")]
    [string]$Mode,
    [Parameter(Mandatory = $true)]
    [object[]]$Entries
  )

  Write-Host ""
  Write-Host "Trench Tools services are ready."
  if ($env:TRENCH_TOOLS_FINAL_DIAGNOSTICS -eq "1") {
    Show-BrowserTunnelGuidance -Mode $Mode
    return
  }
  Write-Host ""
  Write-Host "Launched services:"
  foreach ($entry in $Entries) {
    $displayName = Get-DisplayNameForBinary -BinaryName $entry.Name
    $port = Get-PortForBinary -BinaryName $entry.Name
    Write-Host "  - $displayName ($($entry.Name))"
    Write-Host "    Address: http://127.0.0.1:$port"
    Write-Host "    Process ID: $($entry.ProcessId)"
    Write-Host "    Log file: $($entry.LogPath)"
    Write-Host "    Error log: $($entry.ErrorLogPath)"
  }

  Show-BrowserTunnelGuidance -Mode $Mode

  Write-Host ""
  Write-Host "Extension authentication"
  if ($Mode -eq "ld") {
    Write-Host "  - No Execution Engine auth token is needed because you started LaunchDeck only."
    return
  }

  $tokenFilePath = Get-ExecutionEngineAuthTokenPath
  if ([string]::IsNullOrWhiteSpace($tokenFilePath)) {
    Write-Host "  - The Execution Engine started, but the script could not read the auth token file path."
    Write-Host "  - Check the execution-engine log above, then restart this script if needed."
    return
  }

  Write-Host "  Token file: $tokenFilePath"
  Write-Host "  Paste this token into the extension. Keep it private."
}

function Wait-ForStartedProcessesHealthy {
  param(
    [Parameter(Mandatory = $true)]
    [object[]]$Entries
  )

  if ($Entries.Count -eq 0) {
    return
  }

  $pending = New-Object System.Collections.Generic.List[object]
  foreach ($entry in $Entries) {
    $pending.Add($entry) | Out-Null
  }

  for ($attempt = 0; $attempt -lt 120; $attempt++) {
    for ($index = $pending.Count - 1; $index -ge 0; $index--) {
      $entry = $pending[$index]
      switch ($entry.Name) {
        "execution-engine" { $url = "http://127.0.0.1:$($env:EXECUTION_ENGINE_PORT)/api/extension/auth/bootstrap" }
        "launchdeck-engine" { $url = "http://127.0.0.1:$($env:LAUNCHDECK_PORT)/health" }
        "launchdeck-follow-daemon" { $url = "http://127.0.0.1:$($env:LAUNCHDECK_FOLLOW_DAEMON_PORT)/health" }
        default {
          $pending.RemoveAt($index)
          continue
        }
      }

      $healthy = $false
      try {
        $response = Invoke-RestMethod -UseBasicParsing $url -TimeoutSec 2
        $healthy = if ($entry.Name -eq "execution-engine") {
          $response.authRequired -eq $true -or $response.status -eq "ready"
        } else {
          $response.ok -eq $true -or $response.running -eq $true
        }
      } catch {
      }

      if ($healthy) {
        $pending.RemoveAt($index)
        continue
      }

      $process = Get-Process -Id $entry.ProcessId -ErrorAction SilentlyContinue
      if (-not $process) {
        throw "$($entry.Name) exited before it became healthy. Check $($entry.LogPath)."
      }
    }

    if ($pending.Count -eq 0) {
      return
    }

    Start-Sleep -Milliseconds 500
  }

  $details = ($pending | ForEach-Object { "$($_.Name): $($_.LogPath)" }) -join "; "
  throw "Timed out waiting for healthy services. Check $details."
}

function Stop-TrackedProcess {
  param(
    [Parameter(Mandatory = $true)]
    [int]$ProcessId,
    [Parameter(Mandatory = $true)]
    [string]$Label
  )

  if ($ProcessId -le 0 -or $ProcessId -eq $PID) {
    return
  }

  $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
  if (-not $process) {
    return
  }

  try {
    Stop-Process -Id $ProcessId -ErrorAction Stop
  } catch {
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $process) {
      return
    }
  }

  for ($attempt = 0; $attempt -lt 20; $attempt++) {
    Start-Sleep -Milliseconds 500
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $process) {
      return
    }
  }

  Stop-Process -Id $ProcessId -Force -ErrorAction SilentlyContinue
}

function Start-DetachedBinary {
  param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryName,
    [Parameter(Mandatory = $true)]
    [string]$BinaryPath,
    [Parameter(Mandatory = $true)]
    [string]$StdoutLogPath,
    [Parameter(Mandatory = $true)]
    [string]$StderrLogPath
  )

  Rotate-Log -LogPath $StdoutLogPath
  Rotate-Log -LogPath $StderrLogPath
  New-Item -ItemType File -Path $StdoutLogPath -Force | Out-Null
  New-Item -ItemType File -Path $StderrLogPath -Force | Out-Null
  $process = Start-Process `
    -FilePath $BinaryPath `
    -WorkingDirectory $projectRoot `
    -RedirectStandardOutput $StdoutLogPath `
    -RedirectStandardError $StderrLogPath `
    -WindowStyle Hidden `
    -PassThru

  return [pscustomobject]@{
    Name = $BinaryName
    ProcessId = $process.Id
    LogPath = $StdoutLogPath
    ErrorLogPath = $StderrLogPath
  }
}

function Start-LogViewerTerminal {
  param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryName,
    [Parameter(Mandatory = $true)]
    [string]$LogPath,
    [Parameter(Mandatory = $true)]
    [string]$RunDirectory
  )

  $tailScriptPath = Join-Path $projectRoot "scripts\tail-log.ps1"
  if (-not (Test-Path $tailScriptPath)) {
    throw "Missing log viewer script at $tailScriptPath."
  }

  $viewerPidFile = Join-Path $RunDirectory "$BinaryName.log-viewer.pid"
  $marker = Get-LogViewerMarker -BinaryName $BinaryName
  $viewer = Start-Process `
    -FilePath "powershell" `
    -WorkingDirectory $projectRoot `
    -ArgumentList @(
      "-NoProfile",
      "-ExecutionPolicy", "Bypass",
      "-File", ('"{0}"' -f $tailScriptPath),
      "-ViewerKey", $marker,
      "-LogPath", ('"{0}"' -f $LogPath),
      "-Title", ('"{0}"' -f "$BinaryName logs")
    ) `
    -PassThru
  [System.IO.File]::WriteAllText($viewerPidFile, [string]$viewer.Id)
}

$cliMode = Parse-ModeArgument -Arguments $args
Load-DotEnv
$mode = Get-ValidatedMode -RequestedMode $cliMode
$terminalMode = Get-ValidatedTerminalMode -RequestedMode $env:TRENCH_TOOLS_TERMINALS

if ([string]::IsNullOrWhiteSpace($env:EXECUTION_ENGINE_PORT)) { $env:EXECUTION_ENGINE_PORT = "8788" }
if ([string]::IsNullOrWhiteSpace($env:LAUNCHDECK_PORT)) { $env:LAUNCHDECK_PORT = "8789" }
if ([string]::IsNullOrWhiteSpace($env:LAUNCHDECK_FOLLOW_DAEMON_PORT)) { $env:LAUNCHDECK_FOLLOW_DAEMON_PORT = "8790" }
if ([string]::IsNullOrWhiteSpace($env:TRENCH_TOOLS_DATA_ROOT)) { $env:TRENCH_TOOLS_DATA_ROOT = ".local/trench-tools" }
if ([string]::IsNullOrWhiteSpace($env:LOG_DIR)) { $env:LOG_DIR = ".local/logs" }

$dataRoot = Resolve-PathFromProject -RawPath $env:TRENCH_TOOLS_DATA_ROOT
$logDir = Resolve-PathFromProject -RawPath $env:LOG_DIR

if ([string]::IsNullOrWhiteSpace($env:LAUNCHDECK_LOCAL_DATA_DIR)) {
  $env:LAUNCHDECK_LOCAL_DATA_DIR = $dataRoot
} else {
  $env:LAUNCHDECK_LOCAL_DATA_DIR = Resolve-PathFromProject -RawPath $env:LAUNCHDECK_LOCAL_DATA_DIR
}
if ([string]::IsNullOrWhiteSpace($env:LAUNCHDECK_SEND_LOG_DIR)) {
  $env:LAUNCHDECK_SEND_LOG_DIR = Join-Path $dataRoot "send-reports"
} else {
  $env:LAUNCHDECK_SEND_LOG_DIR = Resolve-PathFromProject -RawPath $env:LAUNCHDECK_SEND_LOG_DIR
}
if ([string]::IsNullOrWhiteSpace($env:LAUNCHDECK_ENGINE_RUNTIME_PATH)) {
  $env:LAUNCHDECK_ENGINE_RUNTIME_PATH = Join-Path $dataRoot "engine-runtime.json"
} else {
  $env:LAUNCHDECK_ENGINE_RUNTIME_PATH = Resolve-PathFromProject -RawPath $env:LAUNCHDECK_ENGINE_RUNTIME_PATH
}
if ([string]::IsNullOrWhiteSpace($env:LAUNCHDECK_FOLLOW_DAEMON_STATE_PATH)) {
  $env:LAUNCHDECK_FOLLOW_DAEMON_STATE_PATH = Join-Path $dataRoot "follow-daemon-state.json"
} else {
  $env:LAUNCHDECK_FOLLOW_DAEMON_STATE_PATH = Resolve-PathFromProject -RawPath $env:LAUNCHDECK_FOLLOW_DAEMON_STATE_PATH
}

$env:TRENCH_TOOLS_MODE = $mode
$env:TRENCH_TOOLS_TERMINALS = $terminalMode
$env:TRENCH_TOOLS_DATA_ROOT = $dataRoot
$env:TRENCH_TOOLS_PROJECT_ROOT = $projectRoot
$env:LOG_DIR = $logDir
$env:EXECUTION_ENGINE_BASE_URL = if ([string]::IsNullOrWhiteSpace($env:EXECUTION_ENGINE_BASE_URL)) {
  "http://127.0.0.1:$($env:EXECUTION_ENGINE_PORT)"
} else {
  $env:EXECUTION_ENGINE_BASE_URL
}
if ([string]::IsNullOrWhiteSpace($env:LAUNCHDECK_EXECUTION_ENGINE_BASE_URL)) {
  if ($mode -eq "ld") {
    Remove-Item Env:LAUNCHDECK_EXECUTION_ENGINE_BASE_URL -ErrorAction SilentlyContinue
  } else {
    $env:LAUNCHDECK_EXECUTION_ENGINE_BASE_URL = $env:EXECUTION_ENGINE_BASE_URL
  }
} else {
  $env:LAUNCHDECK_EXECUTION_ENGINE_BASE_URL = $env:LAUNCHDECK_EXECUTION_ENGINE_BASE_URL
}
$env:LAUNCHDECK_FOLLOW_DAEMON_URL = if ([string]::IsNullOrWhiteSpace($env:LAUNCHDECK_FOLLOW_DAEMON_URL)) {
  "http://127.0.0.1:$($env:LAUNCHDECK_FOLLOW_DAEMON_PORT)"
} else {
  $env:LAUNCHDECK_FOLLOW_DAEMON_URL
}

$runDir = Join-Path $dataRoot "run"
New-Item -ItemType Directory -Path $runDir -Force | Out-Null
New-Item -ItemType Directory -Path $logDir -Force | Out-Null
New-Item -ItemType Directory -Path $env:LAUNCHDECK_SEND_LOG_DIR -Force | Out-Null

$targets = @(Get-TargetSpecsForMode -Mode $mode)
Show-StartupOverview -Mode $mode -Targets $targets -LogDirectory $logDir

Start-Step -Label "Stop old processes" -Status "WAIT" -Detail "mode $mode"
& (Join-Path $projectRoot "trench-tools-stop.ps1") --mode $mode | Out-Host
Complete-Step

$started = New-Object System.Collections.Generic.List[object]

try {
  Build-Targets -Targets $targets

  Write-Host ""
  Write-Host "Launched"
  Start-Step -Label "Launch services" -Status "WAIT"
  foreach ($target in $targets) {
    $binaryPath = Get-BinaryPath -Binary $target.Binary
    $stdoutLogPath = Join-Path $logDir "$($target.Binary).log"
    $stderrLogPath = Join-Path $logDir "$($target.Binary).stderr.log"
    $displayName = Get-DisplayNameForBinary -BinaryName $target.Binary
    $entry = Start-DetachedBinary `
      -BinaryName $target.Binary `
      -BinaryPath $binaryPath `
      -StdoutLogPath $stdoutLogPath `
      -StderrLogPath $stderrLogPath
    $started.Add($entry) | Out-Null
    $pidFile = Join-Path $runDir "$($target.Binary).pid"
    [System.IO.File]::WriteAllText($pidFile, [string]$entry.ProcessId)
    if ($terminalMode -eq "logs") {
      Start-LogViewerTerminal -BinaryName $target.Binary -LogPath $entry.LogPath -RunDirectory $runDir
      Write-Host "    Opened a log window for $displayName."
    }
    $port = Get-PortForBinary -BinaryName $target.Binary
    Write-Host ("  OK    {0,-24} http://127.0.0.1:{1,-5} pid {2,-8} logs {3}" -f $displayName, $port, $entry.ProcessId, $entry.LogPath)
  }
  Complete-Step

  Write-Host ""
  Start-Step -Label "Wait for readiness" -Status "WAIT"
  Wait-ForStartedProcessesHealthy -Entries $started.ToArray()
  Complete-Step
} catch {
  try {
    & (Join-Path $projectRoot "trench-tools-stop.ps1") --mode $mode | Out-Host
  } catch {
  }
  throw
}

Show-FinalSummary -Mode $mode -Entries $started.ToArray()
