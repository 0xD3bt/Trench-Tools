$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$engineManifestPath = (Join-Path $projectRoot "rust\launchdeck-engine\Cargo.toml").ToLowerInvariant()

function Get-ConfiguredPort {
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
    $legacyMatch = Select-String -Path $filePath -Pattern '^\s*LAUNCHDECK_ENGINE_PORT\s*=\s*(\d+)\s*$' | Select-Object -First 1
    if ($legacyMatch) {
      return [int]$legacyMatch.Matches[0].Groups[1].Value
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

function Stop-OldLaunchDeckRuntime {
  $knownPids = New-Object System.Collections.Generic.HashSet[int]

  $processes = Get-CimInstance Win32_Process | Where-Object {
    $_.ProcessId -ne $PID -and
    $_.CommandLine -and
    (
      $_.CommandLine.ToLowerInvariant().Contains($engineManifestPath) -or
      $_.CommandLine.ToLowerInvariant().Contains("launchdeck-engine") -or
      $_.CommandLine.ToLowerInvariant().Contains("ui-server.js")
    )
  }

  foreach ($process in $processes) {
    if ($knownPids.Add([int]$process.ProcessId)) {
      Stop-LaunchDeckProcess -ProcessId ([int]$process.ProcessId) -Reason "existing LaunchDeck runtime"
    }
  }

  $port = Get-ConfiguredPort
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
      # Server may still be starting.
    }
  }

  return $false
}

function Start-LaunchDeckHost {
  $port = Stop-OldLaunchDeckRuntime
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

  if (Wait-ForEngineHealth -Port $port) {
    Write-Host "LaunchDeck Rust host ready on port $port."
    Start-Process "http://127.0.0.1:$port" | Out-Null
  } else {
    Write-Warning "LaunchDeck host did not report healthy startup on port $port. Check .local\\launchdeck\\engine-error.log if needed."
  }
}

Set-Location $projectRoot
Start-LaunchDeckHost
