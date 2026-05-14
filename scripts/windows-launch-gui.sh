#!/usr/bin/env bash
set -euo pipefail

script='$taskName = "SnappyRefAgentLaunch"
$launch = Join-Path $env:TEMP "launch-snappyref-gui.ps1"
$repo = (Get-Location).Path
$exe = Join-Path $repo "target\release-fast\snappyref.exe"
Get-Process snappyref -ErrorAction SilentlyContinue | Stop-Process -Force
for ($i = 0; $i -lt 50; $i++) {
    if (-not (Get-Process snappyref -ErrorAction SilentlyContinue)) { break }
    Start-Sleep -Milliseconds 100
}
@"
Set-Location ''$repo''
Start-Process -FilePath ''$exe'' -WorkingDirectory ''$repo''
"@ | Set-Content -Encoding UTF8 $launch
$time = "23:59"
schtasks /Delete /TN $taskName /F 2>$null | Out-Null
schtasks /Create /TN $taskName /TR "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$launch`"" /SC ONCE /ST $time /F /IT
schtasks /Run /TN $taskName
for ($i = 0; $i -lt 50; $i++) {
    if (Get-Process snappyref -ErrorAction SilentlyContinue) { break }
    Start-Sleep -Milliseconds 100
}
for ($i = 0; $i -lt 20; $i++) {
    schtasks /Delete /TN $taskName /F 2>$null | Out-Null
    schtasks /Query /TN $taskName 2>$null | Out-Null
    if ($LASTEXITCODE -ne 0) { break }
    Start-Sleep -Milliseconds 250
}
exit 0
'
enc=$(printf '%s' "$script" | iconv -f UTF-8 -t UTF-16LE | base64 | tr -d '\n')
scripts/windows-remote.sh "powershell -NoProfile -EncodedCommand $enc"
