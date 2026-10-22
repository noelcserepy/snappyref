# Leaves exactly one release-fast SnappyRef from the checkout on the desktop.
Get-Process snappyref -ErrorAction SilentlyContinue | Stop-Process -Force
for ($i = 0; $i -lt 50 -and (Get-Process snappyref -ErrorAction SilentlyContinue); $i++) {
    Start-Sleep -Milliseconds 100
}
$exe = Join-Path $env:SNAPPYREF_WINDOWS_REPO 'target\release-fast\snappyref.exe'
Start-Process -FilePath $exe -WorkingDirectory $env:SNAPPYREF_WINDOWS_REPO
Wait-AppWindow | Out-Null
