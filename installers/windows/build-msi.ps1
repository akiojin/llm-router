Param(
    [string]$Arch = "x64",
    [string]$BinariesDir = "..\\..\\dist\\win"
)

$ErrorActionPreference = "Stop"

if (-not (Get-Command candle.exe -ErrorAction SilentlyContinue)) {
    Write-Error "WiX Toolset (candle.exe, light.exe) not found in PATH."
}

Push-Location $PSScriptRoot

$wxs = Join-Path $PSScriptRoot "llm-router.wxs"
$wixobj = "llm-router-$Arch.wixobj"
$msi = "llm-router-$Arch.msi"

candle.exe -dBinariesDir="$BinariesDir" -dProductVersion="1.0.0" -arch "$Arch" -out $wixobj $wxs
light.exe -out $msi $wixobj

Write-Host "Built $msi"

Pop-Location
