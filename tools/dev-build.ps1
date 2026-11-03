# Build the release exe (what the "RUST quickRS" shortcuts run) while quickRS is running from it (D67).
#
# Windows won't overwrite a running exe but will rename it, so target\release\quickrs.exe and cargo's hard link to it
# (target\release\deps\quickrs.exe) are renamed to .old first, then cargo builds new ones in their place. A running
# quickRS sees the new exe and swaps it in by itself (src/devswap.rs): at once, or at the next pause if a song is
# playing, back on the same song at the same time. The .old files are listed for the next start to delete
# (update::cleanup). If the build fails, the renamed files are put back.
#
#   powershell -File tools\dev-build.ps1            (from anywhere; extra arguments go to cargo, e.g. --features x)

$ErrorActionPreference = 'Stop'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$root = Split-Path $PSScriptRoot -Parent
$rel = Join-Path $root 'target\release'

$moved = @()
foreach ($f in @((Join-Path $rel 'quickrs.exe'), (Join-Path $rel 'deps\quickrs.exe'))) {
    if (-not (Test-Path $f)) { continue }
    # a free .old name: an old one from an earlier swap goes if nothing has it open, else the next number
    $old = "$f.old"
    $i = 1
    while (Test-Path $old) {
        try { Remove-Item $old -Force -ErrorAction Stop } catch { $old = "$f.$i.old"; $i++ }
    }
    Rename-Item -LiteralPath $f -NewName (Split-Path $old -Leaf)
    $moved += [pscustomobject]@{ From = $f; To = $old }
}

& cargo build --release -p qrs-app --manifest-path (Join-Path $root 'Cargo.toml') @args
$ok = ($LASTEXITCODE -eq 0)

if (-not $ok) {
    foreach ($m in $moved) {
        if (-not (Test-Path $m.From)) { Rename-Item -LiteralPath $m.To -NewName (Split-Path $m.From -Leaf) }
    }
    Write-Host 'Build failed; the running quickRS is untouched.'
    exit 1
}

if ($moved.Count -gt 0) {
    $staging = Join-Path $rel '.update'
    New-Item -ItemType Directory -Force $staging | Out-Null
    $lines = [string[]]($moved | ForEach-Object { $_.To })
    [IO.File]::AppendAllLines((Join-Path $staging 'leftovers.txt'), $lines, (New-Object Text.UTF8Encoding $false))
}
Write-Host 'Built. A running quickRS swaps it in by itself (at the next pause if a song is playing).'
