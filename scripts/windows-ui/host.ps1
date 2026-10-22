# Runs inside the scheduled task on the signed-in desktop. Runs step.ps1 from
# the checkout with app.ps1's helpers loaded and records its exit status in
# done.txt: the step's `exit` code, or 1 when it throws or writes an error.
$ErrorActionPreference = 'Stop'
$log = Join-Path $PSScriptRoot 'step.log'
$env:SNAPPYREF_UI_CAPTURES = Join-Path $PSScriptRoot 'captures'
$status = 1
try {
    New-Item -ItemType Directory -Force $env:SNAPPYREF_UI_CAPTURES | Out-Null
    $env:SNAPPYREF_WINDOWS_REPO = Get-Content (Join-Path $PSScriptRoot 'repo.txt') -Raw
    Set-Location $env:SNAPPYREF_WINDOWS_REPO
    . (Join-Path $PSScriptRoot 'app.ps1')
    $global:LASTEXITCODE = 0
    & (Join-Path $PSScriptRoot 'step.ps1') *>&1 | Out-File -FilePath $log -Append -Encoding utf8
    $status = [int]$LASTEXITCODE
} catch {
    "ERROR: $_" | Out-File -FilePath $log -Append -Encoding utf8
} finally {
    Set-Content -Path (Join-Path $PSScriptRoot 'done.tmp') -Value $status
    Move-Item -Force (Join-Path $PSScriptRoot 'done.tmp') (Join-Path $PSScriptRoot 'done.txt')
}
