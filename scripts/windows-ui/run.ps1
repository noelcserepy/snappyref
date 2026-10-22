# Runs over SSH. Starts host.ps1 on the signed-in desktop as a scheduled task,
# waits for the exit status it records, prints the step's log and exits with
# that status.
param([Parameter(Mandatory)][int]$TimeoutSeconds)

# Windows PowerShell reads and writes the ANSI and OEM code pages by default;
# everything that crosses to the Mac is UTF-8.
[Console]::OutputEncoding = New-Object Text.UTF8Encoding $false
$taskName = 'SnappyRefAgentUi'
$done = Join-Path $PSScriptRoot 'done.txt'
$log = Join-Path $PSScriptRoot 'step.log'

function Fail([string]$Message, [int]$Code = 1) {
    [Console]::Error.WriteLine($Message)
    exit $Code
}

# The task runs as the SSH account, so its own desktop must be signed in.
# Compare account SIDs: an SSH session's USERDOMAIN can read WORKGROUP while
# the desktop's explorer.exe belongs to the machine's domain.
function Get-OwnerSid($Process) { (Invoke-CimMethod -InputObject $Process -MethodName GetOwnerSid).Sid }
$processes = @(Get-CimInstance Win32_Process)
$account = Get-OwnerSid ($processes | Where-Object ProcessId -eq $PID)
$signedIn = @($processes | Where-Object {
    $_.Name -eq 'explorer.exe' -and $_.SessionId -gt 0 -and (Get-OwnerSid $_) -eq $account
})
if ($signedIn.Count -eq 0) {
    Fail 'No signed-in interactive session is available. Sign in at the PC first.'
}

# One step at a time owns the desktop's mouse and keyboard, and with it the
# task. Windows releases the mutex when this process exits, however it exits.
$desktop = New-Object Threading.Mutex($false, 'Global\SnappyRefAgentUi')
try { $owned = $desktop.WaitOne(0) } catch [Threading.AbandonedMutexException] { $owned = $true }
if (-not $owned) { Fail 'Another step is running on the desktop.' }
# A run that lost its SSH connection can leave its step running.
if ((Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue).State -eq 'Running') {
    Fail "An earlier step is still running. Stop it with Stop-ScheduledTask $taskName."
}

$hostScript = Join-Path $PSScriptRoot 'host.ps1'
$action = New-ScheduledTaskAction -Execute 'powershell.exe' `
    -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$hostScript`""
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
try {
    Start-ScheduledTask -TaskName $taskName
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while (-not (Test-Path $done) -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 250
    }
    $timedOut = -not (Test-Path $done) -and (Get-ScheduledTask -TaskName $taskName).State -eq 'Running'
    if ($timedOut) { Stop-ScheduledTask -TaskName $taskName }
} finally {
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
}

if (Test-Path $log) { Get-Content $log }
if (-not (Test-Path $done)) {
    if ($timedOut) { Fail "Step timed out after $TimeoutSeconds s." 124 }
    Fail 'Step ended without recording an exit status.'
}
$status = (Get-Content $done -Raw).Trim()
if ($status -notmatch '^-?\d+$') { Fail 'Step ended without recording an exit status.' }
exit [int]$status
