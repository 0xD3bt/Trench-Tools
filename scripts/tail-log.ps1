param(
  [Parameter(Mandatory = $true)]
  [string]$ViewerKey,
  [Parameter(Mandatory = $true)]
  [string]$LogPath,
  [string]$Title = "Trench Tools Log Viewer"
)

$ErrorActionPreference = "Stop"

try {
  $Host.UI.RawUI.WindowTitle = $Title
} catch {
}

$logDirectory = Split-Path -Parent $LogPath
if ($logDirectory) {
  New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
}
if (-not (Test-Path $LogPath)) {
  New-Item -ItemType File -Path $LogPath -Force | Out-Null
}

Write-Host "Following $LogPath"
Write-Host "Viewer key: $ViewerKey"
Write-Host "Press Ctrl+C or close this window to stop following."
Write-Host ""

Get-Content -Path $LogPath -Tail 50 -Wait
