#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
encode_powershell() {
  printf '%s' "$1" | iconv -f UTF-8 -t UTF-16LE | base64 | tr -d '\n'
}

# The scheduled task runs as the SSH account, so its own desktop must be signed in.
session_check='$ErrorActionPreference = "Stop"
$signedIn = @(Get-CimInstance Win32_Process | Where-Object {
    $_.Name -eq "explorer.exe" -and $_.SessionId -gt 0
} | ForEach-Object {
    $owner = Invoke-CimMethod -InputObject $_ -MethodName GetOwner
    if ($owner.User -eq $env:USERNAME -and $owner.Domain -eq $env:USERDOMAIN) { $_ }
})
if ($signedIn.Count -eq 0) { exit 3 }
exit 0'
session_enc=$(encode_powershell "$session_check")
if "$script_dir/windows-remote.sh" "powershell -NoProfile -EncodedCommand $session_enc"; then
  :
else
  result=$?
  if [[ "$result" -eq 3 ]]; then
    echo 'No signed-in interactive session is available. Sign in at the PC first.' >&2
    exit 1
  fi
  echo 'Could not check the Windows interactive session.' >&2
  exit "$result"
fi

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
enc=$(encode_powershell "$script")
"$script_dir/windows-remote.sh" "powershell -NoProfile -EncodedCommand $enc"
