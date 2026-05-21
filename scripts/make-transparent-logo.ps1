param(
  [Parameter(Mandatory = $true)][string]$Source,
  [Parameter(Mandatory = $true)][string]$Destination
)

Add-Type -AssemblyName System.Drawing

$src = [System.Drawing.Image]::FromFile($Source)
$srcBmp = New-Object System.Drawing.Bitmap($src)
$w = $srcBmp.Width
$h = $srcBmp.Height

$dst = New-Object System.Drawing.Bitmap($w, $h, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)

$rect = New-Object System.Drawing.Rectangle(0, 0, $w, $h)
$srcData = $srcBmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$dstData = $dst.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::WriteOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)

$len = $srcData.Stride * $h
$buf = New-Object byte[] $len
[System.Runtime.InteropServices.Marshal]::Copy($srcData.Scan0, $buf, 0, $len)

for ($i = 0; $i -lt $len; $i += 4) {
  $b = $buf[$i]
  $g = $buf[$i + 1]
  $r = $buf[$i + 2]
  $lum = [int](($r * 0.299) + ($g * 0.587) + ($b * 0.114))
  if ($lum -gt 255) { $lum = 255 }
  if ($lum -lt 0) { $lum = 0 }
  $buf[$i] = 255
  $buf[$i + 1] = 255
  $buf[$i + 2] = 255
  $buf[$i + 3] = [byte]$lum
}

[System.Runtime.InteropServices.Marshal]::Copy($buf, 0, $dstData.Scan0, $len)
$srcBmp.UnlockBits($srcData)
$dst.UnlockBits($dstData)

$dir = [System.IO.Path]::GetDirectoryName($Destination)
if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }

$dst.Save($Destination, [System.Drawing.Imaging.ImageFormat]::Png)
$src.Dispose()
$srcBmp.Dispose()
$dst.Dispose()
Write-Output ("Wrote " + $Destination + " (" + $w + "x" + $h + ")")
