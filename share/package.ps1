# Builds the version to share (GitHub release, Reddit) and zips it: dist\share\quickRS-<version>-windows-x64.zip.
# It differs from tools\package.ps1 (the owner's own zip) in three ways:
#   - the NUX MG-300 support is left out (built without the "nux" feature, and assets\mg300 isn't included);
#   - there are no keys, only keys.example.json;
#   - it includes the psarc-to-feedpak converter: "quickRS Converter.exe" (a window) and "Convert songs.bat".
# It builds into target\share, so the owner's own target\release build is left alone (and can stay running).
# Run from anywhere: powershell -ExecutionPolicy Bypass -File share\package.ps1
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$root = Split-Path -Parent $here
Push-Location $root
try {
    cargo build --release --target-dir target\share -p qrs-app --no-default-features -p qrs-cli -p qrs-convert
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $bin = Join-Path $root 'target\share\release'
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
    $name = "quickRS-$version"
    $dist = Join-Path $root 'dist\share'
    $out  = Join-Path $dist $name
    if (Test-Path $out) { Remove-Item -Recurse -Force $out }
    New-Item -ItemType Directory -Force (Join-Path $out 'licences') | Out-Null
    Copy-Item (Join-Path $bin 'quickrs.exe'), (Join-Path $bin 'quickrs-cli.exe') $out
    Copy-Item (Join-Path $bin 'quickrs-convert.exe') (Join-Path $out 'quickRS Converter.exe')
    Copy-Item (Join-Path $here 'README.txt'), (Join-Path $here 'keys.example.json'), (Join-Path $here 'make_shortcut.ps1') $out
    # (cmd wants CRLF line endings)
    $bat = (Get-Content -Raw (Join-Path $here 'Convert songs.bat')) -replace "`r?`n", "`r`n"
    [IO.File]::WriteAllText((Join-Path $out 'Convert songs.bat'), $bat, [Text.Encoding]::ASCII)
    Copy-Item LICENSE (Join-Path $out 'licences\quickRS.txt')
    Copy-Item assets\fonts\OFL.txt (Join-Path $out 'licences\Manrope-OFL.txt')
    Copy-Item assets\wem\LICENSE-ww2ogg.txt (Join-Path $out 'licences\ww2ogg.txt')
    # nothing private: no keys, no songs, no NUX pictures
    foreach ($bad in 'keys.json', 'assets', 'config.json', 'userdata.json') {
        if (Test-Path (Join-Path $out $bad)) { throw "$bad must not be in the shared zip" }
    }
    $zip = Join-Path $dist "$name-windows-x64.zip"
    if (Test-Path $zip) { Remove-Item -Force $zip }
    Compress-Archive -Path $out -DestinationPath $zip
    $mb = [math]::Round((Get-Item $zip).Length / 1MB, 1)
    Write-Host "Made $zip ($mb MB)"
} finally {
    Pop-Location
}
