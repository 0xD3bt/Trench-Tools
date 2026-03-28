$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$uiScriptPath = (Join-Path $projectRoot "ui-server.js").ToLowerInvariant()
$engineManifestPath = (Join-Path $projectRoot "rust\launchdeck-engine\Cargo.toml").ToLowerInvariant()

function Get-ConfiguredUiPort {
  $defaultPort = 8789
  foreach ($fileName in @(".env", ".env.local", ".env.example")) {
    $filePath = Join-Path $projectRoot $fileName
    if (-not (Test-Path $filePath)) {
      continue
    }
    $match = Select-String -Path $filePath -Pattern '^\s*LAUNCHDECK_PORT\s*=\s*(\d+)\s*$' | Select-Object -First 1
    if ($match) {
      return [int]$match.Matches[0].Groups[1].Value
    }
  }
  return $defaultPort
}

function Get-ConfiguredEnginePort {
  $defaultPort = 8790
  foreach ($fileName in @(".env", ".env.local", ".env.example")) {
    $filePath = Join-Path $projectRoot $fileName
    if (-not (Test-Path $filePath)) {
      continue
    }
    $match = Select-String -Path $filePath -Pattern '^\s*LAUNCHDECK_ENGINE_PORT\s*=\s*(\d+)\s*$' | Select-Object -First 1
    if ($match) {
      return [int]$match.Matches[0].Groups[1].Value
    }
  }
  return $defaultPort
}

function Stop-LaunchDeckProcess {
  param(
    [Parameter(Mandatory = $true)]
    [int]$ProcessId,
    [Parameter(Mandatory = $true)]
    [string]$Reason
  )

  if ($ProcessId -eq $PID) {
    return
  }

  try {
    Stop-Process -Id $ProcessId -Force -ErrorAction Stop
    Write-Host "Stopped process $ProcessId ($Reason)."
  } catch {
    Write-Warning "Failed to stop process $ProcessId ($Reason): $($_.Exception.Message)"
  }
}

function Stop-OldLaunchDeckUi {
  $knownPids = New-Object System.Collections.Generic.HashSet[int]

  $processes = Get-CimInstance Win32_Process | Where-Object {
    $_.ProcessId -ne $PID -and
    $_.CommandLine -and
    (
      (
        $_.Name -match '^(?i:node(?:\.exe)?)$' -and
        $_.CommandLine.ToLowerInvariant().Contains($uiScriptPath)
      ) -or (
        $_.CommandLine -match '(?i)npm(?:\.cmd)?\s+run\s+ui' -and
        $_.CommandLine.ToLowerInvariant().Contains("launchdeck")
      )
    )
  }

  foreach ($process in $processes) {
    if ($knownPids.Add([int]$process.ProcessId)) {
      Stop-LaunchDeckProcess -ProcessId ([int]$process.ProcessId) -Reason "existing LaunchDeck UI"
    }
  }

  $port = Get-ConfiguredUiPort
  $netstatOutput = netstat -ano -p tcp | Select-String -Pattern "127\.0\.0\.1:$port\s+.*LISTENING\s+(\d+)$"
  foreach ($line in $netstatOutput) {
    if ($line.Matches.Count -eq 0) {
      continue
    }
    $matchedPid = [int]$line.Matches[0].Groups[1].Value
    if ($knownPids.Add($matchedPid)) {
      Stop-LaunchDeckProcess -ProcessId $matchedPid -Reason "listener on port $port"
    }
  }

  return $port
}

function Stop-OldLaunchDeckEngine {
  $knownPids = New-Object System.Collections.Generic.HashSet[int]

  $processes = Get-CimInstance Win32_Process | Where-Object {
    $_.ProcessId -ne $PID -and
    $_.CommandLine -and
    (
      $_.CommandLine.ToLowerInvariant().Contains($engineManifestPath) -or
      $_.CommandLine.ToLowerInvariant().Contains("launchdeck-engine")
    )
  }

  foreach ($process in $processes) {
    if ($knownPids.Add([int]$process.ProcessId)) {
      Stop-LaunchDeckProcess -ProcessId ([int]$process.ProcessId) -Reason "existing LaunchDeck engine"
    }
  }

  $port = Get-ConfiguredEnginePort
  $netstatOutput = netstat -ano -p tcp | Select-String -Pattern "127\.0\.0\.1:$port\s+.*LISTENING\s+(\d+)$"
  foreach ($line in $netstatOutput) {
    if ($line.Matches.Count -eq 0) {
      continue
    }
    $matchedPid = [int]$line.Matches[0].Groups[1].Value
    if ($knownPids.Add($matchedPid)) {
      Stop-LaunchDeckProcess -ProcessId $matchedPid -Reason "engine listener on port $port"
    }
  }

  return $port
}

function Wait-ForEngineHealth {
  param(
    [Parameter(Mandatory = $true)]
    [int]$Port
  )

  for ($attempt = 0; $attempt -lt 20; $attempt++) {
    Start-Sleep -Milliseconds 500
    try {
      $response = Invoke-RestMethod -UseBasicParsing "http://127.0.0.1:$Port/health" -TimeoutSec 2
      if ($response.ok -eq $true) {
        return $true
      }
    } catch {
      # Engine may still be starting.
    }
  }

  return $false
}

function Start-LaunchDeckEngine {
  $enginePort = Stop-OldLaunchDeckEngine
  $logDir = Join-Path $projectRoot ".local\launchdeck"
  New-Item -ItemType Directory -Path $logDir -Force | Out-Null
  $stdoutPath = Join-Path $logDir "engine.log"
  $stderrPath = Join-Path $logDir "engine-error.log"

  Start-Process `
    -FilePath "cargo" `
    -ArgumentList @("run", "--manifest-path", "rust/launchdeck-engine/Cargo.toml", "--bin", "launchdeck-engine") `
    -WorkingDirectory $projectRoot `
    -WindowStyle Hidden `
    -RedirectStandardOutput $stdoutPath `
    -RedirectStandardError $stderrPath | Out-Null

  if (Wait-ForEngineHealth -Port $enginePort) {
    Write-Host "LaunchDeck engine ready on port $enginePort."
  } else {
    Write-Warning "LaunchDeck engine did not report healthy startup on port $enginePort. Check .local\\launchdeck\\engine-error.log if needed."
  }
}

Set-Location $projectRoot
Start-LaunchDeckEngine
$port = Stop-OldLaunchDeckUi
Write-Host "Starting LaunchDeck UI on port $port..."
& npm run ui
