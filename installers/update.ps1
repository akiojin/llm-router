Param(
    [string]$Repo = "akiojin/ollama-router"
)

$scriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path
& (Join-Path $scriptPath "install.ps1") -Repo $Repo

Write-Host "Update completed."
