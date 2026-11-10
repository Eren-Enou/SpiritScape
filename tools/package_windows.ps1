# Builds a folder you can copy anywhere and play from, with no Rust needed:
#
#     powershell -ExecutionPolicy Bypass -File tools\package_windows.ps1
#
# It lands in dist\SpiritScape-<branch>\. Saves stay in %APPDATA%\SpiritScape,
# so a new copy picks up where the last one left off.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

cargo build --release -p spirit_game
if ($LASTEXITCODE -ne 0) { throw 'The build failed. The messages above say why.' }

$branch = (git rev-parse --abbrev-ref HEAD).Trim()
$out = Join-Path $root "dist\SpiritScape-$branch"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force $out | Out-Null

Copy-Item 'target\release\spirit_game.exe' $out
Copy-Item -Recurse 'assets\models' (Join-Path $out 'assets\models')
Copy-Item -Recurse 'assets\audio' (Join-Path $out 'assets\audio')
Copy-Item 'play.bat', 'scenarios.bat' $out
Copy-Item 'docs\PLAYTEST.md' $out

Write-Host "Packaged into $out"
