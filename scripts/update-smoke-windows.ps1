$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repoRoot

$smokeRoot = if ($env:SNAPPYREF_UPDATE_SMOKE_ROOT) {
    $env:SNAPPYREF_UPDATE_SMOKE_ROOT
} else {
    Join-Path $repoRoot "target\update-smoke\windows"
}
$smokeRoot = [System.IO.Path]::GetFullPath($smokeRoot)

if ([string]::IsNullOrWhiteSpace($smokeRoot) -or $smokeRoot -eq [System.IO.Path]::GetPathRoot($smokeRoot) -or $smokeRoot -eq $repoRoot) {
    throw "refusing unsafe smoke root: $smokeRoot"
}

$installRoot = Join-Path $smokeRoot "SnappyRef"
$statusPath = Join-Path $smokeRoot "status.txt"
$timeoutSecs = if ($env:SNAPPYREF_UPDATE_SMOKE_TIMEOUT_SECS) { [int]$env:SNAPPYREF_UPDATE_SMOKE_TIMEOUT_SECS } else { 300 }
$helperTimeoutSecs = if ($env:SNAPPYREF_UPDATE_SMOKE_HELPER_TIMEOUT_SECS) { [int]$env:SNAPPYREF_UPDATE_SMOKE_HELPER_TIMEOUT_SECS } else { 120 }

if (Test-Path -LiteralPath $smokeRoot) {
    Remove-Item -LiteralPath $smokeRoot -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $installRoot | Out-Null

cargo build --profile release-fast --locked --no-default-features --features semantic-search --bins
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$targetDir = Join-Path $repoRoot "target\release-fast"
$appExe = Join-Path $installRoot "snappyref.exe"
$helperExe = Join-Path $installRoot "snappyref-updater.exe"
Copy-Item (Join-Path $targetDir "snappyref.exe") $appExe
Copy-Item (Join-Path $targetDir "snappyref-updater.exe") $helperExe

$process = New-Object System.Diagnostics.Process
$process.StartInfo.FileName = $appExe
$process.StartInfo.Arguments = "--update-smoke-install --no-startup-focus"
$process.StartInfo.WorkingDirectory = $installRoot
$process.StartInfo.UseShellExecute = $false
$process.StartInfo.EnvironmentVariables["SNAPPYREF_UPDATE_CURRENT_VERSION"] = if ($env:SNAPPYREF_UPDATE_CURRENT_VERSION) { $env:SNAPPYREF_UPDATE_CURRENT_VERSION } else { "0.0.0" }
$process.StartInfo.EnvironmentVariables["SNAPPYREF_INSTALL_ROOT"] = $installRoot
$process.StartInfo.EnvironmentVariables["SNAPPYREF_LIBRARY_ROOT"] = Join-Path $smokeRoot "library"
$process.StartInfo.EnvironmentVariables["SNAPPYREF_UPDATE_SMOKE_STATUS"] = $statusPath
$process.StartInfo.EnvironmentVariables["SNAPPYREF_UPDATE_SMOKE_TIMEOUT_SECS"] = [string]$timeoutSecs
if ($env:SNAPPYREF_UPDATE_MANIFEST_URL) {
    $process.StartInfo.EnvironmentVariables["SNAPPYREF_UPDATE_MANIFEST_URL"] = $env:SNAPPYREF_UPDATE_MANIFEST_URL
}

[void]$process.Start()
if (-not $process.WaitForExit($timeoutSecs * 1000)) {
    $process.Kill()
    throw "smoke app timed out after $timeoutSecs seconds"
}
if ($process.ExitCode -ne 0) {
    throw "smoke app exited with code $($process.ExitCode)"
}

$deadline = (Get-Date).AddSeconds($helperTimeoutSecs)
while ((Get-Date) -lt $deadline) {
    if ((Test-Path -LiteralPath $statusPath) -and ((Get-Content -Raw -Path $statusPath) -match "^failed:")) {
        throw "update smoke failed: $(Get-Content -Raw -Path $statusPath)"
    }
    if ((Test-Path -LiteralPath $appExe) -and (Test-Path -LiteralPath (Join-Path $installRoot "snappyref.exe.old"))) {
        break
    }
    Start-Sleep -Seconds 1
}

if (-not (Test-Path -LiteralPath $appExe)) {
    throw "updated app exe missing at $appExe"
}
if (-not (Test-Path -LiteralPath (Join-Path $installRoot "snappyref.exe.old"))) {
    throw "backup exe missing at $(Join-Path $installRoot "snappyref.exe.old")"
}

Get-CimInstance Win32_Process |
    Where-Object { $_.ExecutablePath -eq $appExe } |
    ForEach-Object { Invoke-CimMethod -InputObject $_ -MethodName Terminate | Out-Null }

Write-Host "Windows update smoke passed"
Write-Host "scratch install: $installRoot"
