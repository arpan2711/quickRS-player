# Build the release exe (what the "RUST quickRS" shortcuts run) while quickRS is running from it (D67, D175).
#
# cargo builds into its own folder (target\devbuild), so target\release\quickrs.exe stays where it is for the whole
# build: the shortcuts keep working. Only after a good build is the new exe put in place: copied next to the old one
# as quickrs.exe.new, then the running exe renamed to .old (Windows allows renaming a running exe, not overwriting
# it) and the .new renamed to quickrs.exe, two renames a split second apart. A running quickRS sees the new exe and
# swaps it in by itself (src/devswap.rs): at once, or at the next pause if a song is playing, back on the same song at
# the same time. The .old file is listed for the next start to delete (update::cleanup). A failed build touches
# nothing in target\release.
#
#   powershell -File tools\dev-build.ps1            (from anywhere; extra arguments go to cargo, e.g. --features x)
#
# For testing the script itself: -ReleaseDir / -TargetDir point it at scratch folders, -Cargo at a stand-in for cargo.

param(
    [string]$ReleaseDir = '',
    [string]$TargetDir = '',
    [string]$Cargo = 'cargo',
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$CargoArgs = @()
)

$ErrorActionPreference = 'Stop'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$root = Split-Path $PSScriptRoot -Parent
if (-not $ReleaseDir) { $ReleaseDir = Join-Path $root 'target\release' }
if (-not $TargetDir) { $TargetDir = Join-Path $root 'target\devbuild' }
# (full paths: update::cleanup only deletes listed files inside the exe's folder)
$ReleaseDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($ReleaseDir)
$TargetDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($TargetDir)

& $Cargo build --release -p qrs-app --manifest-path (Join-Path $root 'Cargo.toml') --target-dir $TargetDir @CargoArgs
$built = Join-Path $TargetDir 'release\quickrs.exe'
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $built)) {
    Write-Host 'Build failed; target\release (and a running quickRS) untouched.'
    exit 1
}

New-Item -ItemType Directory -Force $ReleaseDir | Out-Null
$dest = Join-Path $ReleaseDir 'quickrs.exe'
$new = "$dest.new"
Copy-Item -LiteralPath $built -Destination $new -Force

$old = $null
if (Test-Path -LiteralPath $dest) {
    # a free .old name: an old one from an earlier swap goes if nothing has it open, else the next number
    $old = "$dest.old"
    $i = 1
    while (Test-Path -LiteralPath $old) {
        try { Remove-Item -LiteralPath $old -Force -ErrorAction Stop } catch { $old = "$dest.$i.old"; $i++ }
    }
    Rename-Item -LiteralPath $dest -NewName (Split-Path $old -Leaf)
}
try {
    Rename-Item -LiteralPath $new -NewName 'quickrs.exe'
} catch {
    # (put the old one back so the shortcut still runs something)
    if ($old -and -not (Test-Path -LiteralPath $dest)) { Rename-Item -LiteralPath $old -NewName 'quickrs.exe' }
    Write-Host "Couldn't put the new exe in place: $_"
    exit 1
}

# the symbols too, for crash reports (skipped if something has them open)
$pdb = Join-Path $TargetDir 'release\quickrs.pdb'
if (Test-Path -LiteralPath $pdb) {
    try { Copy-Item -LiteralPath $pdb -Destination (Join-Path $ReleaseDir 'quickrs.pdb') -Force -ErrorAction Stop } catch {}
}

if ($old) {
    $staging = Join-Path $ReleaseDir '.update'
    New-Item -ItemType Directory -Force $staging | Out-Null
    [IO.File]::AppendAllLines((Join-Path $staging 'leftovers.txt'), [string[]]@($old), (New-Object Text.UTF8Encoding $false))
}
Write-Host 'Built. A running quickRS swaps it in by itself (at the next pause if a song is playing).'
