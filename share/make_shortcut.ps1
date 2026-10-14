# Puts a "quickRS" shortcut on your Desktop for the quickrs.exe next to this script.
# Right-click this file > Run with PowerShell. -Name gives the shortcut another name.
param([string]$Name = 'quickRS')
$ErrorActionPreference = 'Stop'
$exe = Join-Path $PSScriptRoot 'quickrs.exe'
if (-not (Test-Path $exe)) { throw "quickrs.exe isn't next to this script" }
$exe     = (Resolve-Path $exe).Path
$desktop = [Environment]::GetFolderPath('Desktop')   # follows OneDrive-redirected Desktops too
$path    = Join-Path $desktop "$Name.lnk"
$shell   = New-Object -ComObject WScript.Shell
$lnk     = $shell.CreateShortcut($path)
$lnk.TargetPath       = $exe
$lnk.WorkingDirectory = Split-Path -Parent $exe
$lnk.IconLocation     = "$exe,0"
$lnk.Description      = 'quickRS Bristol 2026: Rocksmith song library and player'
$lnk.Save()
Write-Host "Shortcut created: $path"
